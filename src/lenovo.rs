//! The fans of Lenovo laptops (Legion, LOQ and others of their line), as
//! their firmware reports them to Lenovo's own software through WMI
//! (root\wmi): on the newer, LENOVO_OTHER_METHOD's GetFeatureValue, asked
//! for each fan's speed by its feature's number; on the older,
//! LENOVO_FAN_METHOD's Fan_GetCurrentFanSpeed, by the fan's number. Which a
//! laptop has is told by its fan table (LENOVO_FAN_TABLE_DATA), the sensors
//! and fans it pairs, or, where no table says, by its fan test data
//! (LENOVO_FAN_TEST_DATA), the fans it names. As Lenovo Legion Toolkit has
//! them (its team's, which knows the laptops of 2025 on: their tables pair
//! the processor's fan with sensor 1, or name a chipset fan besides). A
//! laptop's fans are its embedded controller's; this asks the firmware,
//! which knows, instead of reading the controller (whose registers differ
//! from model to model).

use windows::core::{BSTR, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE};
use windows::Win32::System::Ole::{SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{VARIANT, VT_ARRAY, VT_I1, VT_I2, VT_I4, VT_TYPEMASK, VT_UI1, VT_UI2, VT_UI4};
use windows::Win32::System::Wmi::{IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_FLAG_RETURN_WBEM_COMPLETE, WBEM_INFINITE};

/// How the firmware is asked for a fan's speed: the class and method, the method's argument and its answer.
struct Method {
    class: &'static str,
    method: &'static str,
    argument: &'static str,
    answer: &'static str,
}

/// By a feature's number (the laptops of 2023 on).
const FEATURE: Method = Method { class: "LENOVO_OTHER_METHOD", method: "GetFeatureValue", argument: "IDs", answer: "Value" };
/// By a fan's number (those before).
const FAN: Method = Method { class: "LENOVO_FAN_METHOD", method: "Fan_GetCurrentFanSpeed", argument: "FanID", answer: "CurrentFanSpeed" };

/// The features' numbers of the processor's fan's speed, the graphics', the chipset's; with the names the page
/// translates.
const CPU: (i32, &str) = (0x0403_0001, "cpu_fan");
const GPU: (i32, &str) = (0x0403_0002, "gpu_fan");
const PCH: (i32, &str) = (0x0403_0004, "pch_fan");

/// A kind of laptop: the sensor and fan pairs its fan table has (what tells it is of that kind), how its fans are
/// asked, and what each is asked with.
struct Kind {
    pairs: &'static [(u32, u32)],
    method: &'static Method,
    fans: &'static [(i32, &'static str)],
}

/// In order: a laptop is of the first whose pairs its fan table all has (the ones with a chipset fan named before
/// those that would match them without it).
const KINDS: [Kind; 6] = [
    Kind { pairs: &[(4, 1), (5, 2), (1, 4), (5, 5)], method: &FEATURE, fans: &[CPU, GPU, PCH] },
    Kind { pairs: &[(1, 1), (5, 2), (4, 4)], method: &FEATURE, fans: &[CPU, GPU, PCH] },
    Kind { pairs: &[(1, 1), (5, 2), (4, 1)], method: &FEATURE, fans: &[CPU, GPU] },
    Kind { pairs: &[(4, 1), (5, 2)], method: &FEATURE, fans: &[CPU, GPU] },
    Kind { pairs: &[(3, 0), (4, 1)], method: &FAN, fans: &[(0, "cpu_fan"), (1, "gpu_fan")] },
    Kind { pairs: &[(0, 0), (0, 1)], method: &FAN, fans: &[(0, "cpu_fan"), (1, "gpu_fan")] },
];

/// A laptop whose fan table says none of the above, whose fan test data names the processor's fan or the
/// graphics' (fans 1 and 2).
const TESTED: Kind = Kind { pairs: &[], method: &FEATURE, fans: &[CPU, GPU] };

/// A laptop of a kind neither its table nor its test data tells: its firmware asked by each method in turn, the
/// newer first, for the processor's fan and the graphics', and taken to be of the first whose method is there and
/// answers for a fan.
const ASKED: [Kind; 2] = [TESTED, Kind { pairs: &[], method: &FAN, fans: &[(0, "cpu_fan"), (1, "gpu_fan")] }];

pub struct LenovoFans {
    services: IWbemServices,
    method: &'static Method,
    /// What told the kind of laptop, for a report.
    told: String,
    /// The object the method is asked of, and the method's input, to be filled in.
    path: BSTR,
    input: IWbemClassObject,
    /// The fans that answered.
    fans: Vec<(i32, &'static str)>,
}

impl LenovoFans {
    /// The laptop's fans, if it is a Lenovo one whose firmware reports them. Else, for a Lenovo, what was found of
    /// them (for a report), none for another maker's.
    pub fn open() -> Result<Self, Option<String>> {
        // (Asked only of a Lenovo: WMI takes a while to say a class is not there.)
        let maker = crate::metrics::reg_string(windows::core::w!(r"HARDWARE\DESCRIPTION\System\BIOS"), windows::core::w!("SystemManufacturer"));
        if !maker.trim().eq_ignore_ascii_case("lenovo") {
            return Err(None);
        }
        let services = services(r"root\wmi").ok_or_else(|| Some("Lenovo firmware: its WMI could not be reached".to_string()))?;
        let table = table(&services);
        let tested = tested(&services);
        let found: Vec<(&Kind, String)> = match KINDS.iter().find(|kind| kind.pairs.iter().all(|pair| table.contains(pair))) {
            Some(kind) => vec![(kind, format!("fan table {}", shown(kind.pairs)))],
            None if tested.iter().any(|fan| [1, 2].contains(fan)) => vec![(&TESTED, format!("fan test data, fans {tested:?}"))],
            None => ASKED.iter().map(|kind| (kind, format!("{} (fan table {}; fan test data: fans {tested:?})", kind.method.method, shown(&table)))).collect(),
        };
        let mut said = Vec::new();
        for (kind, told) in found {
            match Self::asked(&services, kind, told) {
                Ok(fans) => return Ok(fans),
                Err(why) => said.push(why),
            }
        }
        Err(Some(said.join("; ")))
    }

    /// The laptop's fans, taken to be of `kind` (told so by `told`): those that answer its method. Else what was
    /// found of them, for a report.
    fn asked(services: &IWbemServices, kind: &Kind, told: String) -> Result<Self, String> {
        let found = |what: &str| format!("Lenovo firmware, by its {told}: {what}");
        let class = BSTR::from(kind.method.class);
        // The object the method is asked of: the class's one instance.
        let objects = unsafe { services.CreateInstanceEnum(&class, WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_WBEM_COMPLETE, None) }.map_err(|_| found("no LENOVO method class"))?;
        let mut instance = [None];
        let mut returned = 0u32;
        let _ = unsafe { objects.Next(WBEM_INFINITE, &mut instance, &mut returned) };
        let object = instance[0].take().filter(|_| returned == 1).ok_or_else(|| found("no instance of its method class"))?;
        let path = get(&object, "__PATH").and_then(|path| BSTR::try_from(&path).ok()).ok_or_else(|| found("no path to its method class"))?;
        // The method's input, from the class's definition of it.
        let mut definition = None;
        unsafe { services.GetObject(&class, Default::default(), None, Some(&mut definition), None) }.map_err(|_| found("its method class undefined"))?;
        let (mut input, mut output) = (None, None);
        let method: Vec<u16> = kind.method.method.encode_utf16().chain([0]).collect();
        unsafe { definition.ok_or_else(|| found("its method class undefined"))?.GetMethod(PCWSTR(method.as_ptr()), 0, &mut input, &mut output) }.map_err(|_| found("no such method"))?;
        let input = input.ok_or_else(|| found("no input to its method"))?;
        let mut fans = LenovoFans { services: services.clone(), method: kind.method, told: told.clone(), path, input, fans: Vec::new() };
        fans.fans = kind.fans.iter().copied().filter(|(asked, _)| fans.ask(*asked).is_some_and(|speed| speed >= 0)).collect();
        if fans.fans.is_empty() {
            return Err(found("no fan answered"));
        }
        Ok(fans)
    }

    /// Each fan's speed (RPM), by name.
    pub fn read(&self) -> Vec<(String, f32)> {
        self.fans.iter().filter_map(|(asked, name)| Some((name.to_string(), self.ask(*asked)?.max(0) as f32))).collect()
    }

    /// What the firmware answers the method asked with `value`.
    fn ask(&self, value: i32) -> Option<i64> {
        let input = unsafe { self.input.SpawnInstance(0) }.ok()?;
        let argument: Vec<u16> = self.method.argument.encode_utf16().chain([0]).collect();
        // (A uint32 goes to WMI as a 32-bit integer.)
        unsafe { input.Put(PCWSTR(argument.as_ptr()), 0, &VARIANT::from(value), 0) }.ok()?;
        let mut output = None;
        unsafe { self.services.ExecMethod(&self.path, &BSTR::from(self.method.method), Default::default(), None, &input, Some(&mut output), None) }.ok()?;
        number(&get(output.as_ref()?, self.method.answer)?)
    }

    /// The fans found, for a report.
    pub fn describe(&self) -> String {
        format!("Lenovo firmware, by its {}: {}", self.told, self.fans.iter().map(|(_, name)| *name).collect::<Vec<_>>().join(", "))
    }
}

/// WMI's namespace `namespace` (root\wmi is where the firmware's classes are).
fn services(namespace: &str) -> Option<IWbemServices> {
    let locator: IWbemLocator = unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let empty = BSTR::new();
    let services = unsafe { locator.ConnectServer(&BSTR::from(namespace), &empty, &empty, &empty, 0, &empty, None) }.ok()?;
    unsafe { CoSetProxyBlanket(&services, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, None, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE) }.ok()?;
    Some(services)
}

/// Every object of a query's answer.
fn every(services: &IWbemServices, query: &str) -> Vec<IWbemClassObject> {
    let Ok(objects) = (unsafe { services.ExecQuery(&BSTR::from("WQL"), &BSTR::from(query), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None) }) else { return Vec::new() };
    let mut all = Vec::new();
    loop {
        let mut found = [None];
        let mut returned = 0u32;
        let _ = unsafe { objects.Next(WBEM_INFINITE, &mut found, &mut returned) };
        match found[0].take().filter(|_| returned == 1) {
            Some(object) => all.push(object),
            None => return all,
        }
    }
}

/// The sensor and fan pairs of the laptop's fan table, each once, in order.
fn table(services: &IWbemServices) -> Vec<(u32, u32)> {
    let mut pairs: Vec<(u32, u32)> = every(services, "SELECT Sensor_ID, Fan_Id FROM LENOVO_FAN_TABLE_DATA")
        .iter()
        .filter_map(|row| Some((u32::try_from(number(&get(row, "Sensor_ID")?)?).ok()?, u32::try_from(number(&get(row, "Fan_Id")?)?).ok()?)))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// The fans the laptop's fan test data names, each once, in order.
fn tested(services: &IWbemServices) -> Vec<i64> {
    let mut fans: Vec<i64> = every(services, "SELECT FanId FROM LENOVO_FAN_TEST_DATA").iter().filter_map(|row| get(row, "FanId")).flat_map(|ids| numbers(&ids)).collect();
    fans.sort_unstable();
    fans.dedup();
    fans
}

/// Pairs as a report writes them.
fn shown(pairs: &[(u32, u32)]) -> String {
    if pairs.is_empty() {
        return "empty".into();
    }
    pairs.iter().map(|(sensor, fan)| format!("{sensor}-{fan}")).collect::<Vec<_>>().join(" ")
}

fn get(object: &IWbemClassObject, name: &str) -> Option<VARIANT> {
    let mut value = VARIANT::default();
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    unsafe { object.Get(PCWSTR(name.as_ptr()), 0, &mut value, None, None) }.ok()?;
    Some(value)
}

/// A whole number, whichever way WMI gives it.
fn number(value: &VARIANT) -> Option<i64> {
    i32::try_from(value).map(i64::from).or_else(|_| u32::try_from(value).map(i64::from)).or_else(|_| i64::try_from(value)).ok()
}

/// The whole numbers of an array, whichever width WMI gives them in (none, if it is not an array of them).
fn numbers(value: &VARIANT) -> Vec<i64> {
    let vt = value.vt();
    if vt.0 & VT_ARRAY.0 == 0 {
        return Vec::new();
    }
    let array = unsafe { value.Anonymous.Anonymous.Anonymous.parray };
    if array.is_null() {
        return Vec::new();
    }
    let (Ok(low), Ok(high)) = (unsafe { SafeArrayGetLBound(array, 1) }, unsafe { SafeArrayGetUBound(array, 1) }) else { return Vec::new() };
    (low..=high)
        .filter_map(|index| {
            // Each element read into eight bytes, as wide as its type (little-endian).
            let mut element = 0u64;
            unsafe { SafeArrayGetElement(array, &index, (&mut element as *mut u64).cast()) }.ok()?;
            Some(match vt.0 & VT_TYPEMASK.0 {
                t if t == VT_UI1.0 => element as u8 as i64,
                t if t == VT_I1.0 => element as u8 as i8 as i64,
                t if t == VT_UI2.0 => element as u16 as i64,
                t if t == VT_I2.0 => element as u16 as i16 as i64,
                t if t == VT_UI4.0 => element as u32 as i64,
                t if t == VT_I4.0 => element as u32 as i32 as i64,
                _ => return None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "asks this machine's firmware"]
    fn reads_the_fans() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let started = std::time::Instant::now();
        let fans = super::LenovoFans::open();
        println!("{:?} in {:?}", fans.as_ref().map(|fans| (fans.describe(), fans.read())).map_err(|why| why.clone()), started.elapsed());
    }

    /// An array WMI gives (the BIOS's characteristics, numbers of 16 bits) read whole.
    #[test]
    #[ignore = "asks this machine's WMI"]
    fn reads_an_array() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let services = super::services(r"root\cimv2").unwrap();
        let rows = super::every(&services, "SELECT BiosCharacteristics FROM Win32_BIOS");
        let read = super::numbers(&super::get(&rows[0], "BiosCharacteristics").unwrap());
        println!("{read:?}");
        assert!(!read.is_empty() && read.iter().all(|n| (0..64).contains(n)));
    }

    /// The kinds are told apart as Lenovo Legion Toolkit's team tells them: the first whose pairs are all there.
    #[test]
    fn tells_a_laptop_by_its_fan_table() {
        let kind = |table: &[(u32, u32)]| super::KINDS.iter().position(|kind| kind.pairs.iter().all(|pair| table.contains(pair)));
        // A Legion of 2025 whose processor's fan is paired with sensor 1, and the chipset's fan with sensor 4.
        assert_eq!(kind(&[(1, 1), (4, 4), (5, 2)]), Some(1));
        // One without the chipset's fan.
        assert_eq!(kind(&[(1, 1), (4, 1), (5, 2)]), Some(2));
        // Those of 2023 and 2024.
        assert_eq!(kind(&[(4, 1), (5, 2)]), Some(3));
        assert_eq!(kind(&[(1, 4), (4, 1), (5, 2), (5, 5)]), Some(0));
        // Older ones.
        assert_eq!(kind(&[(3, 0), (4, 1)]), Some(4));
        assert_eq!(kind(&[(0, 0), (0, 1)]), Some(5));
        // A table of none of them (its fan test data then says).
        assert_eq!(kind(&[(1, 1), (5, 2)]), None);
    }
}
