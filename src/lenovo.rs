//! The fans of Lenovo laptops (Legion, LOQ and others of their line), as
//! their firmware reports them to Lenovo's own software through WMI
//! (root\wmi): on those of 2024 on, LENOVO_OTHER_METHOD's GetFeatureValue,
//! asked for each fan's speed by its feature's number; on those before,
//! LENOVO_FAN_METHOD's Fan_GetCurrentFanSpeed, by the fan's number. Which a
//! laptop has is told by its fan table (LENOVO_FAN_TABLE_DATA): the sensors
//! and fans it pairs. As Lenovo Legion Toolkit has them (its sensors'
//! controllers, versions 1 to 3). A laptop's fans are its embedded
//! controller's; this asks the firmware, which knows, instead of reading
//! the controller (whose registers differ from model to model).

use windows::core::{BSTR, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_FLAG_RETURN_WBEM_COMPLETE, WBEM_INFINITE};

/// How a generation of Lenovo's firmware is asked for its fans: which sensor and fan its fan table pairs for the
/// processor and for the graphics (what tells it is that generation), the class and method asked, the method's
/// argument and answer, and what it is asked with for each fan, with the name the page translates.
struct Generation {
    number: u8,
    pairs: [(u32, u32); 2],
    class: &'static str,
    method: &'static str,
    argument: &'static str,
    answer: &'static str,
    fans: [(i32, &'static str); 2],
}

/// Newest first: a laptop is of the first whose pairs its fan table has.
const GENERATIONS: [Generation; 3] = [
    // The features' numbers: the processor's fan's speed, the graphics'.
    Generation { number: 3, pairs: [(4, 1), (5, 2)], class: "LENOVO_OTHER_METHOD", method: "GetFeatureValue", argument: "IDs", answer: "Value", fans: [(0x0403_0001, "cpu_fan"), (0x0403_0002, "gpu_fan")] },
    Generation { number: 2, pairs: [(3, 0), (4, 1)], class: "LENOVO_FAN_METHOD", method: "Fan_GetCurrentFanSpeed", argument: "FanID", answer: "CurrentFanSpeed", fans: [(0, "cpu_fan"), (1, "gpu_fan")] },
    Generation { number: 1, pairs: [(0, 0), (0, 1)], class: "LENOVO_FAN_METHOD", method: "Fan_GetCurrentFanSpeed", argument: "FanID", answer: "CurrentFanSpeed", fans: [(0, "cpu_fan"), (1, "gpu_fan")] },
];

pub struct LenovoFans {
    services: IWbemServices,
    generation: &'static Generation,
    /// The object the method is asked of, and the method's input, to be filled in.
    path: BSTR,
    input: IWbemClassObject,
    /// The fans that answered.
    fans: Vec<(i32, &'static str)>,
}

impl LenovoFans {
    /// The laptop's fans, if it is a Lenovo one whose firmware reports them.
    pub fn open() -> Option<Self> {
        // (Asked only of a Lenovo: WMI takes a while to say a class is not there.)
        let maker = crate::metrics::reg_string(windows::core::w!(r"HARDWARE\DESCRIPTION\System\BIOS"), windows::core::w!("SystemManufacturer"));
        if !maker.trim().eq_ignore_ascii_case("lenovo") {
            return None;
        }
        let locator: IWbemLocator = unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let empty = BSTR::new();
        let services = unsafe { locator.ConnectServer(&BSTR::from(r"root\wmi"), &empty, &empty, &empty, 0, &empty, None) }.ok()?;
        unsafe { CoSetProxyBlanket(&services, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, None, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE) }.ok()?;
        let generation = GENERATIONS.iter().find(|generation| generation.pairs.iter().all(|(sensor, fan)| pairs(&services, *sensor, *fan)))?;
        let class = BSTR::from(generation.class);
        // The object the method is asked of: the class's one instance.
        let objects = unsafe { services.CreateInstanceEnum(&class, WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_WBEM_COMPLETE, None) }.ok()?;
        let mut found = [None];
        let mut returned = 0u32;
        let _ = unsafe { objects.Next(WBEM_INFINITE, &mut found, &mut returned) };
        let object = found[0].take().filter(|_| returned == 1)?;
        let path = BSTR::try_from(&get(&object, "__PATH")?).ok()?;
        // The method's input, from the class's definition of it.
        let mut definition = None;
        unsafe { services.GetObject(&class, Default::default(), None, Some(&mut definition), None) }.ok()?;
        let (mut input, mut output) = (None, None);
        let method: Vec<u16> = generation.method.encode_utf16().chain([0]).collect();
        unsafe { definition?.GetMethod(PCWSTR(method.as_ptr()), 0, &mut input, &mut output) }.ok()?;
        let mut fans = LenovoFans { services, generation, path, input: input?, fans: Vec::new() };
        fans.fans = generation.fans.into_iter().filter(|(asked, _)| fans.ask(*asked).is_some_and(|speed| speed >= 0)).collect();
        (!fans.fans.is_empty()).then_some(fans)
    }

    /// Each fan's speed (RPM), by name.
    pub fn read(&self) -> Vec<(String, f32)> {
        self.fans.iter().filter_map(|(asked, name)| Some((name.to_string(), self.ask(*asked)?.max(0) as f32))).collect()
    }

    /// What the firmware answers the method asked with `value`.
    fn ask(&self, value: i32) -> Option<i64> {
        let input = unsafe { self.input.SpawnInstance(0) }.ok()?;
        let argument: Vec<u16> = self.generation.argument.encode_utf16().chain([0]).collect();
        // (A uint32 goes to WMI as a 32-bit integer.)
        unsafe { input.Put(PCWSTR(argument.as_ptr()), 0, &VARIANT::from(value), 0) }.ok()?;
        let mut output = None;
        unsafe { self.services.ExecMethod(&self.path, &BSTR::from(self.generation.method), Default::default(), None, &input, Some(&mut output), None) }.ok()?;
        number(&get(output.as_ref()?, self.generation.answer)?)
    }

    /// The fans found, for a report.
    pub fn describe(&self) -> String {
        format!("Lenovo firmware (generation {}): {}", self.generation.number, self.fans.iter().map(|(_, name)| *name).collect::<Vec<_>>().join(", "))
    }
}

/// Whether the laptop's fan table pairs sensor `sensor` with fan `fan`.
fn pairs(services: &IWbemServices, sensor: u32, fan: u32) -> bool {
    let query = BSTR::from(format!("SELECT * FROM LENOVO_FAN_TABLE_DATA WHERE Sensor_ID = {sensor} AND Fan_Id = {fan}"));
    let Ok(objects) = (unsafe { services.ExecQuery(&BSTR::from("WQL"), &query, WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None) }) else { return false };
    let mut found = [None];
    let mut returned = 0u32;
    let _ = unsafe { objects.Next(WBEM_INFINITE, &mut found, &mut returned) };
    returned == 1
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
        println!("{:?} in {:?}", fans.as_ref().map(|fans| (fans.describe(), fans.read())), started.elapsed());
    }
}
