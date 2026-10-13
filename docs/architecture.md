# Glance's processes and what each may do

Status: proposal, fifth draft, for review before it is built. "Now" marks how
the code stands where that differs. "Evidence" marks what was tried, and
where its log is. The answers to the second review are listed at the end.

## Stage 1, as it is being built

Four review rounds made the rest of this document thorough; not all of it is
worth its cost for a desktop monitor. Stage 1 builds, in this order:

1. No start path runs the app's UI elevated (the shell hand-off).
2. The settings' switches and reports off the panel thread.
3. The pipe: owner Administrators and the narrow user ACE, both ends
   verified (the client through the pipe at identification level), the
   bootstrap with package versions.
4. The service's log lines sent to the app, which alone writes the log.
   Docker, WSL and the microphone stay in the service: WSL's readings need
   the rights (the VM's command line, GPU use by process), and the others,
   in memory-safe Rust, gain too little from moving to be worth a second
   reading pipeline.
5. Updates: the signer by Artifact Signing's profile OID and the root's hash;
   the installer's own version read and compared.
6. The installer: tasks disabled, processes stopped and seen gone, files
   replaced, tasks restored; a failure is fixed by running it again.

All six are built. Deferred until something shows they are needed: the
maintenance record and its phased recovery, the watchdog's table (stage 1
keeps one rule: a sample stuck for 30 s ends the service, and the app starts
it again), revocation special cases, leaked-buffer
accounting, the queues' saturation rules (a full queue drops the connection),
and the drag controller's finer association rules (stage 2).

## Scope decisions (the owner's)

- **One sign-in session per account.** Several accounts signed in at once
  (fast user switching) are supported, each with its own app and service.
  The same account signed in twice at once (possible only on multi-session
  Windows) is not: the second app finds the first's single-instance object
  for its account (`Global\Glance.App.<build>.<SID>`), says that Glance runs
  in another sign-in of this account, and exits.
- Keep the user-facing side simple; this document is about the parts the user
  never sees.

## Why more than one process

Some readings need administrator rights: the board's chip (temperatures,
fans, voltages), CPU temperatures and power, memory temperatures (all through
PawnIO), SATA drives' SMART data and standby state (ATA pass-through), ASUS
boards' fans (`root\wmi`), and the frames games present (a real-time ETW
session). Running those without a prompt at every start needs an elevated
task registered once. Starting Glance at sign-in as such needs no rights.

Everything else must run as the user: drops onto elevated windows are refused
by Windows; programs started from an elevated process run elevated; app
notifications are unavailable to elevated processes; shell dialogs load
third-party extensions; and the parsers of lyrics, covers, Docker's API,
notes and dropped files must not run with an administrator's rights.

## Threat model

- Any process of the same user can open the pipe, start the genuine
  `glance.exe` or `glance-sensors.exe` with any arguments, and tamper with the app. Verifying the
  client narrows who talks to the service; it does not make requests
  trustworthy. So each capability below states the worst such a process can
  do with it, and is bounded so that worst is acceptable.
- The service parses only its own bounded protocol (below) and verifies
  downloaded installers before running them; it reads no other file, URL or
  command it is given.
- What the service tells the app is data shown, never a path opened or a
  command run.
- Out of scope: administrator or kernel attackers; physical access; UAC turned
  off (then every process of the user is elevated and the split protects
  nothing: Glance runs as one process, as before; see "Starting").

## The processes

| Process | Rights | How many | Does |
| --- | --- | --- | --- |
| `glance.exe` (the app) | the user's, medium integrity | one per account | every window; tray and shortcuts; the readings needing no rights merged with the service's; media, lyrics, covers; Docker, WSL, microphone; the widgets' window watch; update checks; reports; launching and opening; drops; the journal |
| `glance-sensors.exe` (the service) | elevated, same account | one per app | privileged readings; presented frames; global input observation; the capabilities below |
| `glance-remote.exe` (the phone helper) | the user's | one per machine | the phone's network side, media keys and their key-injection fallback, OpenRGB, ASUS keyboard lighting (HID), the cooler's display, monitor power and brightness, power plans, the screen nudge |

## The service's capabilities

| Capability | Message | Worst a same-user process can do | Bounds |
| --- | --- | --- | --- |
| Privileged readings | `Want { interval_ms, refresh_slow }` | read sensors the user can already see | interval 250–10000 ms |
| Game mode | `GameMode(Auto/On/Off)` | change which game's frames are reported | 3 values |
| Edges watched | `Edges { screens, rest, pressure }` | make pushes open the panel or not | ≤ 32 screens, names ≤ 128 UTF-16 units, pressure 20–2000; the frame ≤ 64 KiB (worst-case JSON escaping of every name fits) |
| Input observation | `Observe { buttons: bool, activity: bool }` | learn when the mouse is pressed, when a device was used (never which key) | booleans |
| Report | `Report` | read chip names and read counts | ≤ 1 per 2 s |
| Autostart | `Switch { Autostart, on }` | turn Glance's own tasks on or off | fixed task names and definitions (below); ≤ 1 per s; only from a protected installation |
| Phone remote | `Switch { Remote, on }` | open or close the helper's fixed firewall rule; register or remove the helper's task | the rule names the helper in the protected install folder and the configured port; only the helper's machine-wide owner session may turn it off; ≤ 1 per s; protected installation only |
| Update | `Install { version, installers }` | install a genuine newer Glance (a deliberate delegation) | "Updates", below; protected installation only; one at a time; ≤ 1 per 10 min |
| Retire | bootstrap flag | stop the service | none needed |

"Protected installation": the install folder passes the existing check
(`elevation::may_start_unasked`: the path resolved, and the executable and
each ancestor folder writable only by administrators, SYSTEM or
TrustedInstaller; Program Files qualifies, so does any folder protected so). A
service running from anywhere else (a build folder) refuses `Switch` and
`Install` with `Refused(unprotected)`; readings and input still work.

One installation per machine is supported: the installer finds an existing
one through its uninstall registration and installs over it (or refuses a
second folder). The debug build uses its own names (a `Debug` suffix) and is
never installed.

### Updates

- `version` is `major.minor.patch`, each ≤ 65535, parsed first. It is only a
  wish: the service trusts no version the app states. The installer is saved
  as `Glance-update.exe` in the service's protected download folder
  (`%ProgramData%\Glance\update`, administrators only); no request string
  reaches a file name.
- **Its version is read from the file itself**: after the signer checks below
  pass, the service reads the installer's `VERSIONINFO` (fixed file version)
  from the verified file. It must equal `version` and exceed the installed
  `glance.exe`'s file version; else the file is deleted and the next candidate
  tried. The installer checks the installed version again, authoritatively,
  under the installation lock (no lock is held by the service, so nothing
  waits on anything).
- **The bytes run are the bytes checked**: the service keeps the file open
  with read sharing only (no writer, no deleter) from before verification
  until the installer process it starts has exited; the folder is
  administrators-only besides.
- Hosts: `https://glancepc.com/…`;
  `https://github.com/lulu-loopp/glance/releases/download/…`, redirects only to
  `objects.githubusercontent.com` or `release-assets.githubusercontent.com`;
  `https://gitee.com/lulu-loopp/glance/releases/download/…`, redirects only to
  the gitee asset host recorded when stage 1 is built (by following a real
  release URL; the list is in code, not configuration). Each redirect parsed and
  checked; HTTPS only; ≤ 4 candidates; URL ≤ 1024 chars; ≤ 64 MiB; ≤ 120 s each.
- **Signer**: `WinVerifyTrust` (`WINTRUST_ACTION_GENERIC_VERIFY_V2`,
  revocation checked for the whole chain) returns success. One failure is
  tolerated, deliberately (a fail-open choice for machines that cannot reach
  the revocation servers): `CERT_E_REVOCATION_FAILURE` /
  `CRYPT_E_REVOCATION_OFFLINE`, and only when the same verification run again
  with revocation checking off succeeds, the signature carries a valid
  timestamp, and every check below passes. Any other failure rejects the file. Then, on the
  signer of **that accepted signature** (taken from WinVerifyTrust's own
  provider data, not from a second parse of the file): the chain's root is the
  certificate with SHA-256
  `5367F20C7ADE0E2BCA790915056D086B720C33C1FA2A2661ACF787E3292E1270`
  (Microsoft Identity Verification Root Certificate Authority 2020; matched by
  that hash, not its name); the leaf's subject is
  `CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, S=mi, C=US`; its extended key
  usages contain code signing, `1.3.6.1.4.1.311.97.1.0` (Artifact Signing)
  and the profile's own OID
  `1.3.6.1.4.1.311.97.921771326.840678362.737210289.98417136`. Artifact
  Signing renews the leaf daily (72-hour validity); the profile OID stays, and
  only this account's profile carries it. Evidence: `D:\Glance\glance.exe`
  (0.2.7) and its chain, read 2026-10-13. The leaf is not pinned. **Rotation**:
  a new profile's OID is added to the list in code by a release signed under
  the current profile; an incoming installer never authorizes its own signer.
  Now: subject names are compared.
- **No downgrade**: the version must exceed the installed `glance.exe`'s file
  version, read under the installation lock (below) by the installer too.

## Global input

### Evidence (VM, 2026-10-13, `target/evidence/capprobe-elevated-front.txt`)

An unelevated topmost window with `WS_EX_NOACTIVATE` that answers
`MA_NOACTIVATE`, an elevated window in front; a press on it, `SetCapture`,
the pointer moved out and the button let go (input injected by an elevated
process). Observed: 32 and 34 moves outside the window delivered to it in the
two runs, `WM_LBUTTONUP` delivered on release over the elevated window and
over the desktop; meanwhile `GetAsyncKeyState(VK_LBUTTON)` in the unelevated
process read "up" throughout. Not tested: physical devices, capture moved to
another window, pens. To confirm with the owner's mouse before stage 2 ships.

### The rule

With a verified, live service: the app reads no global input state (no
`GetAsyncKeyState`, raw input registration, low-level or WinEvent hooks).
Allowed: messages to its own windows (including the button state carried in
`WM_MOUSEMOVE`'s `wParam`), `GetKeyState` for the message being handled,
registered hotkeys, `GetCursorPos`, window geometry. Without a service the app
reads the buttons and the mouse's raw input itself (what it can: nothing over
an elevated window), with the same code.

### As built (stage 2)

- Drags (the panel, widgets, the overlay) end on their own window's
  `WM_LBUTTONUP`, its capture lost, or a `WM_MOUSEMOVE` without `MK_LBUTTON`;
  the panel's own release after a tear-off lets the widget go. No broker
  `Button` is needed for drags: the capture brings the release.
- The service tells `Buttons { down, pressed }`: on each raw mouse report
  with a button going down (`pressed`, so a click shorter than the panel's
  look is not missed) or whenever the state read with `GetAsyncKeyState`
  changes, and once as the app comes. The panel counts presses since it last
  looked; one counted, or a button down, away from the panel closes it.
- Activity: `Asked::Activity(bool)` while the helper has the screens out
  (asked again on each new link), `Told::Activity { touched, nothing }` at
  most every 50 ms or when its kind changes; the app writes them to
  `Local\Glance.Touches` as before. A process hears a device kind at one
  window only, so the window that hears the mouse for the edges (the
  service's, or the panel's without one) hears it for activity too, and only
  the other kinds are registered for activity.

### The broker (in the service; the same code in the app without one)

One thread, one hidden window, one raw-input registration per device class.
Subscribers: edge pushes (`Pushed { edge, at, seq }`), button transitions
(`Button { button, down, at, tick, seq }`, primary/secondary after
`SM_SWAPBUTTON`, read at each transition), activity (below). `tick` is the raw
input message's time (`GetMessageTime`, ms, `u32`, wrapping compared by
signed difference). On every connection, and whenever observation resumes, the
service sends `Buttons { primary, secondary, middle }` read with
`GetAsyncKeyState` in the service (elevated: it reads truly). Observation is
scoped to the service's session's input desktop; while the secure desktop
(UAC, lock screen) has the input, it sends `Observation { available: false }`.

**Activity** keeps the helper's existing interface unchanged: the shared
memory `Local\Glance.Touches` holds `{"touched": [device, tick] | null,
"nothing": [device, tick] | null}` (as `touches.rs` writes and
`remote/src/reads.rs` reads it now), `device` the raw-input device's name
(≤ 256 chars), `tick` `GetTickCount` milliseconds (u32, wrapping; the helper
compares as now). The service keeps the latest meaningful and the latest no-op
report and sends both whenever either changes (at most every 50 ms):
`Activity { touched, nothing }`; the app writes them as they come. The app
asks `Observe { activity: true }` only while the helper has the screens out
(`Glance.Remote.Out`), and again on every new connection. When observation
becomes unavailable, the link is lost, or activity is turned off, the app
writes both as `null` at once (the helper's "nothing heard"); the app exiting
closes the mapping. A protocol change to this interface would be versioned in
both programs together.

### Dismissal on a press elsewhere

On `Button { primary or secondary, down: true }`, the app tests `at` against
its own windows' rectangles (shadows and click-through areas excluded, as
`Panel::track` does now); outside them, and with no drag in progress, the
open panel closes. A press whose `tick` precedes the panel's opening is
ignored.

### Drags

One `Drags` controller on the panel thread owns every drag; native messages
reach it by window.

1. **Begin**: `WM_LBUTTONDOWN` on a Glance window: a drag `{ id, window,
   pressed_tick = GetMessageTime() }`, `SetCapture(window)`.
2. **Follow**: `WM_MOUSEMOVE` and the follow timer, `GetCursorPos`.
3. **End** (geometry kept, the release's actions done) on the first of:
   `WM_LBUTTONUP` to the drag's current or former window; a `WM_MOUSEMOVE` to
   it whose `wParam` lacks `MK_LBUTTON`; `Button { primary, down: false }`
   with `tick` after `pressed_tick` (signed difference).
4. **Cancel** (last good geometry kept, no click, no fling) on: capture taken
   by another window that is not the drag's next owner (`WM_CAPTURECHANGED`
   handled synchronously in the window procedure, which records the new
   capture holder from `lParam` and the drag id at that moment); the window
   hidden or destroyed; `Observation { available: false }` (the input desktop
   changed; capture is lost then anyway); a queue resync (below).
5. **Hand-off** (a widget torn off the panel): the controller makes the
   widget, sets the drag's next owner to it, calls `SetCapture(widget)` (the
   panel's `WM_CAPTURECHANGED` names the widget: a transfer, not a cancel),
   and moves the drag to the widget, all within one message's handling. A
   `WM_LBUTTONUP` or button-less `WM_MOUSEMOVE` that arrives for the panel
   afterwards ends the drag, wherever it is. If the release came before the
   hand-off (the panel's `WM_LBUTTONUP` handled first), the drag has already
   ended and no widget is made. If making the widget fails, the drag is
   cancelled and the panel stays.
6. A window releases capture only for the drag it holds; ending an older
   drag never touches a newer one's capture.
7. **Terminal latch and reentrancy**: ending or cancelling marks the drag
   terminal *first*; the controller's own `ReleaseCapture` then produces a
   `WM_CAPTURECHANGED` it ignores. Every call that can dispatch messages
   (making a window, `SetCapture`, `SetWindowPos`) is followed by a check that
   the drag is still the current, non-terminal one; a release during the
   widget's creation leaves the tear-off unmade. After `SetCapture(widget)`
   the controller confirms `GetCapture() == widget` (`SetCapture` returns the
   previous holder, not success); otherwise it cancels.
8. **Stale messages**: a native message to a former owner ends a drag only
   if that drag is still the newest and the message's time
   (`GetMessageTime`) is not before its `pressed_tick`; otherwise it is
   ignored. A broker `Button` release is associated with the drag whose press
   was of the same physical button (the broker reports the physical button
   and the mapping at the press; the release takes the press's mapping), with
   `tick` at or after `pressed_tick`. When association is uncertain (a resync,
   a new observation epoch), the drag is cancelled, never ended.
9. **Neither release arrives** (no service, and capture lost silently): a
   drag with no mouse message for 10 s and the pointer where it was is
   cancelled.

Release positions are screen coordinates (`GetCursorPos` at the message, or
the broker's `at`), never a message's client coordinates.

Without the service, step 3's third trigger is absent; the first two remain,
and step 9 bounds what is left. The experiment saw release delivered; that is
evidence, not a guarantee. **Lyric windows** move and size in Windows' own
non-client loop (`HTCAPTION` and the sizing hit codes), outside the
controller; their place is kept on `WM_EXITSIZEMOVE`.

### Outbound actions others may refuse

Best effort, each result checked, nothing escalated to the service:
bringing a game back after the panel closes (`SetForegroundWindow` may be
refused by the foreground rules; `SC_RESTORE` posted to a window of higher
integrity is dropped by UIPI: the two independently); the helper's
`SendInput` (key fallback, screen nudge: a short insert count is reported to
the phone as not done); the helper ending other lighting software
(`OpenProcess(PROCESS_TERMINATE)` failing is reported as "close it
yourself").

## Readings: who reads what, and how they meet

The service sends `Readings` (in `link::Told`), each field as the structures
in `reading.rs` have it (units: °C, rpm, V, W):

| Field | Source |
| --- | --- |
| `cpu_sensors: Option<CpuSensors>` | PawnIO (temperatures, package power) |
| `board: Option<BoardSensors>` | PawnIO Super I/O chip; ASUS boards' fans through `root\wmi` merged in |
| `dimm_temps: Vec<f32>` | PawnIO, SMBus, slot order |
| `sata: Vec<SataReading { device: u32, serial: String, celsius: Option<f32>, standby: bool }>` | ATA pass-through |
| `presenting: Vec<Presenting>`, `mode` | ETW, DXGI and D3D9 `Present_Start` |
| `taken_ms: u64` | the service's clock at the read (milliseconds since its start); the app keeps its own receipt time |

The app reads everything else in `Sample` itself and fills these fields from
the latest `Readings`; `drive_temps` takes the storage-property temperature
where Windows gives one, else the matching `sata` entry (by device number and
serial). A privileged field older than 2.5 × the interval shows as absent.
`StaticInfo` is the app's (CPU and board names come from SMBIOS and the
registry, which need no rights); the service's `Hello` adds `found: Vec<String>`
(the chips it found) for the settings and reports. Game selection stays the
app's (`presents::mirror` now). **Now** the service runs the whole sampler and
sends whole samples.

**Logs**: the service writes no file in the user's profile; its lines go to
the app as `Note(String ≤ 400 chars)`, the last 200 kept in memory and sent to
the next app. **Now** both write `glance.log`.

## The link

### The pipe

- Name `\\.\pipe\glance-<build>-<SID>`, `build` `release` or `debug` (one
  installation per machine).
- Made by the service with the security descriptor
  `O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x0012008B;;;<user SID>)S:(ML;;NW;;;ME)`:
  owner Administrators (so no implicit owner rights for the user), the user
  only `FILE_GENERIC_READ | FILE_WRITE_DATA` (no `FILE_CREATE_PIPE_INSTANCE`,
  no `WRITE_DAC`); `FILE_FLAG_FIRST_PIPE_INSTANCE`, `FILE_FLAG_OVERLAPPED`,
  `PIPE_REJECT_REMOTE_CLIENTS`, one instance.
- The app opens it with `CreateFileW(name, FILE_GENERIC_READ |
  FILE_WRITE_DATA, 0, NULL (not inheritable), OPEN_EXISTING,
  FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
  NULL)`.
- **Evidence** (VM, 2026-10-13, `target/evidence/pipeexp-*.txt`): with that
  descriptor, an unelevated process of the same user was refused making a
  second instance of the name (error 5) and opening it for `WRITE_DAC` (error
  5), and connected with the access above; the service read the client's
  user SID, session 1 and integrity `0x2000` through the pipe at
  identification level, and reverted.
- **Squatting**: the name existing before the service makes it means another
  process holds it. The service then exits, writing why to
  `%ProgramData%\Glance\service-refused.log` (administrators write, users read);
  the app finds a server that fails verification, stops trying for this run and
  says in the settings that another program holds Glance's pipe. Nothing is
  ended.

### Verifying the other end

- **The service, of its client**, after reading the client's bootstrap
  (impersonation needs a read first): `ImpersonateNamedPipeClient`,
  `OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, TRUE /* OpenAsSelf */,
  …)` (as the experiment does), the token's facts read, then `RevertToSelf`
  on every path (its result checked; failure ends the service). Any failure to
  impersonate, open or query is a rejection. The token's user SID must equal
  the service's own, its integrity be exactly medium (`0x2000`), its session
  (`GetNamedPipeClientSessionId`, and the token's) the service's. Then, as
  screening, the client's process (`GetNamedPipeClientProcessId` →
  `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)`, the handle
  kept) must be unsignalled and its image this install's `glance.exe` (both
  paths resolved; a failure is a mismatch). The kept handle's signal ends the
  connection.
- **The app, of its server**, before sending more than the bootstrap:
  `GetNamedPipeServerProcessId` → `OpenProcess(QUERY_LIMITED | SYNCHRONIZE)`
  (kept); the process unsignalled, its token's user SID the app's, elevated
  and of high integrity, its session the app's, its image this install's
  `glance-sensors.exe`. A squatter can be the app's own user and session, but not
  elevated: that check is the one that cannot be faked from below.

### Bootstrap

16 bytes each way, little-endian: magic `GLNC` (4), protocol version (u16),
build kind (u8: 0 release, 1 debug), flags (u8: bit 0 retire; other bits must
be 0), package version (3 × u16: major, minor, patch), reserved (2, must be
0). Order: the app sends; the service verifies the client (above), checks the
fields, answers with its own; the app verifies the server and checks the
answer. **The app and the service must be of the same package version**: a
different protocol, build kind or package version and the service answers and
closes; the app then connects again with the retire flag (every service from
this version on exits within 2 s on it, whatever its own version) and starts
the service of its own version through the task. The two-pipe link of the
unreleased split builds never shipped; released versions (≤ 0.3.7) have no
service to meet.

### Frames and queues

- `u32` LE length, then a JSON message. Limits per kind (serialized bytes):
  `Readings` 256 KiB, `Report` 64 KiB, `Edges` 64 KiB, `Install` 16 KiB (4
  URLs of ≤ 1024 chars, escaped, fit), `Note` 2 KiB, every other message
  4 KiB; a larger frame ends the connection. Each semantic limit is set so that
  a valid message, escaped in the worst case, fits its frame (unit-tested per
  kind). Request ids are assigned when a request is put in the queue, after any
  coalescing.
- Buffers left to an I/O that never completed (see the deadline rule) are at
  most one per direction per connection, and the app makes at most 3 new
  connections before treating the service as hung (so at most 6 such buffers
  of ≤ 256 KiB).
- **Deadlines** (absolute, from the event that starts them): connecting 5 s;
  verification and bootstrap, 5 s from connection; a frame, 2 s from its first
  byte (an idle connection between frames is fine); writing a frame, 2 s.
  Each direction has its own `OVERLAPPED` and buffer; a deadline missed:
  `CancelIoEx`, then wait for the operation's completion
  (`GetOverlappedResult(…, TRUE)`) before the buffer or structure is reused or
  freed, then disconnect. That wait is itself watched (see "Liveness"): an I/O
  that does not complete within 5 s of its cancellation ends the process (the
  service) or abandons the connection's buffers to the leaked operation (the
  app, which then makes a new connection with new buffers).
- **Queues**: each side has one writer thread and a queue of at most 1 MiB in
  all. Coalesced (only the latest waits): `Readings`, `Edges`, `Want`,
  `Activity`. Ordered, ≤ 64 waiting: `Button`, `Pushed`; overflow drops them
  all and puts a `Resync` control message in at once, on which the app cancels
  any drag (no click, no fling) and treats buttons as up until the next
  `Buttons` (which the service sends right after `Resync`). Control and reply
  messages (`Hello`, `Buttons`, `Observation`, `Resync`, `Alive`, replies)
  have their own 32 places; since at most 8 requests are outstanding and
  `Alive`, `Buttons` and `Observation` are coalesced, they cannot fill in a
  healthy connection, and if they ever do the connection is closed (both sides
  then reconnect). Producers never block: a full queue refuses at once and the
  producer applies these rules. No thread that draws waits on the link.

### Requests

`{ id: u32, asked: Asked }`, ids strictly increasing per connection (a
connection is a generation: nothing crosses from one to the next); an id not
above the last one seen is answered `Refused(duplicate)` (bounded state: one
number). At most 8 outstanding; a 9th is refused at once. Answers:
`Done { id, result }` or `Refused { id, why }`. Long work runs on the
service's workers (one per kind), each with an overall deadline, after which
it stops its own work (closes connections, deletes partial files) and answers
`Refused(timeout)`: a switch 15 s; a report 2 s; an update 300 s for all
candidates together (each ≤ 120 s). A switch that timed out may or may not
have taken effect: its answer says `unknown`, and the app reads the state anew
(`State`). The app waits the work's deadline + 5 s; a reply after that, or
after the connection changed, is dropped, and the app reads the state anew.
Nothing is replayed, except, on a new connection, `Want`, `Edges`,
`GameMode` and `Observe` (desired state); `Install` never.

### Liveness

- The service sends `Alive` every second, from its writer, independent of
  sampling. 5 s without anything from it (30 s after a resume from sleep), and
  the app treats it as hung.
- **The service's watchdog** (its own thread) ends the service's process if:
  initialization takes over 30 s; verification and bootstrap over 5 s; a
  sample over 10 s; any read, write or connect not completing within 5 s of
  its cancellation; a worker over its overall deadline + 10 s; the input
  broker's, the reader's or the writer's loop not coming round for 10 s (each
  loop stamps a progress time and wakes at least every second: the reader's
  pending read is waited on with a 1 s timeout, the writer stamps also while
  its queue is empty, so a writer stuck before it issues I/O is seen). Its clock is
  `QueryUnbiasedInterruptTime` (sleep excluded). It covers stalls inside the
  process; one it cannot see (the watchdog itself stopped) leaves the app with
  a silent service, which it drops (below) and replaces once the old one dies.
- **Freshness of readings**: each group in `Readings` carries the time it was
  acquired (`cpu_at`, `board_at`, `dimm_at`, `sata_at`, `frames_at`); a cached
  slow reading keeps its acquisition time, not the frame's.

| From | To | Within |
| --- | --- | --- |
| the service's last message | the app treating it as hung | 5 s (30 s after resume) |
| a sample, I/O or loop stall | the watchdog ending the process | the stall's limit above + ≤ 1 s |
| initialization stalled | the process ended | 31 s |
| a worker stalled | the process ended | its deadline + 11 s (update: 311 s) |
| the process's death | the app's first restart | 1 s (then the backoff) |

## Lifecycle

### Starting

- **No path runs the app's UI elevated while UAC is on.** An elevated
  `glance.exe` (a task of ≤ 0.3.7, a build run by
  hand, the installer): if this account's app runs, it starts the service's
  own program beside it (elevated as it is) and exits;
  if not, it hands the start to the desktop shell: `IShellWindows::
  FindWindowSW(SWC_DESKTOP)` → the desktop's `IShellBrowser` → its view →
  `IShellFolderViewDual::get_Application` → `IShellDispatch2::ShellExecute(
  "<install>\glance.exe", "--handed-off")` (the existing Explorer's own
  automation object, so the app starts with Explorer's token). Before calling,
  it checks that Explorer's process (the shell window's) is of its own user
  SID and session and of medium integrity; otherwise it refuses. It then
  starts the service's own program beside it (elevated as it is) and exits.
  An app started with `--handed-off` that finds itself elevated never hands off
  again: it exits. With no suitable shell, the elevated process exits without
  any UI and without retrying.
- **UAC off**: for an administrator account the shell is itself elevated and
  there is no medium token; Glance then runs as one elevated process, as up to
  0.3.7, and the split protects nothing. For a standard account UAC off grants
  no rights: the app runs as the user without the service.
- **Sign-in**: the task `Glance at sign-in <SID>` (LeastPrivilege, logon
  trigger for this user) starts the app; turning it on or off needs no rights.
- **The service**: the app runs the task `Glance <SID>` with
  `IRegisteredTask::Run`. Its definition: principal the user's SID (from the
  service's token at registration), `LogonType InteractiveToken`, `RunLevel
  HighestAvailable`; no triggers; `AllowStartOnDemand`, `MultipleInstancesPolicy
  IgnoreNew` (one session per account), no execution time limit; one action
  `"<install>\glance-sensors.exe" --for <SID>`. It is registered by the
  elevated service with `TASK_DONT_ADD_PRINCIPAL_ACE` and the security
  descriptor `O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;<user SID>)`
  (owner Administrators, so the user has no implicit owner rights; the user may
  read and run it, not change it), then read back, definition and descriptor,
  and compared. Success of a start means a verified connection within 60 s;
  `Run` returning is not success. Evidence so far: the VM's medium app's `Run`
  of the current task succeeded and the service connected (2026-10-13); the new
  definition and descriptor are to be verified the same way when built. A
  standard account's HighestAvailable runs unelevated: the service finds
  itself not elevated and exits; the app runs without one.
- **Task names** stay as now (`Glance <SID>`, `Glance at sign-in <SID>`,
  `Glance Remote <SID>`): one installation per machine; the debug build uses
  `Glance Debug …`.
- **Without the task**: the app asks once (`runas`, the same arguments), at
  first run or when the user turns on something that needs it; a refusal is
  remembered and nothing asks again unasked.
- **Another account's credentials**: a service whose own token's SID differs
  from `--for` exits before doing anything else (no task, no driver, no file);
  the app, its process gone without a connection, says that this account's
  administrator rights are needed and runs without the service.

### Running

- **Service died** (its kept handle signalled): restarted after 1, 2, 5, 10,
  30 s; after 5 deaths in 10 minutes, no more, and the settings say so.
- **Service hung**: the app drops the link, shows privileged readings absent,
  and waits for the watchdog to end it (the times in "Liveness"), then
  restarts it as above.
- **App left**: the service waits 20 s for it, then exits; `retire` at once.
- **Sleep**: the link stays; timers pause; on resume the app sends `Want`.
- **Lock, fast user switching**: the service reports observation unavailable;
  drags cancel; each account's app and service go on in their own session.
- **Sign-out**: the app exits; its service exits 20 s later at most (Windows
  ends both with the session anyway).

### What is per account, and what per machine

- Per account (names carry the build kind and SID): the pipe, the app's
  single-instance object, the tasks, the frames' ETW session
  (`Glance.Frames.<build>.<SID>`; a service stops only the session it holds
  the handle of, or one of exactly its own name left by a crashed service of
  this account), the touches' shared memory and events
  (`Local\`, per session).
- Per machine: the phone helper, and with it the devices it drives. Its
  lock is a file the installer makes, `%ProgramData%\Glance\remote.lock`
  (owner Administrators; SYSTEM and Administrators full; Authenticated Users
  `FILE_GENERIC_READ | FILE_WRITE_DATA` only: no delete, no `WRITE_DAC`, no
  attribute writes), since a medium helper cannot make an
  administrator-owned object itself. Before reading its configuration or
  touching any device, the helper opens it with exactly `FILE_GENERIC_READ |
  FILE_WRITE_DATA` and `FILE_SHARE_READ`: success makes it the owner for as
  long as it keeps the handle; it truncates and writes `{ account SID,
  session, pid, version }` (≤ 512 bytes). Another helper's same open fails
  with a sharing violation; it then opens the file read-only with
  `FILE_SHARE_READ | FILE_SHARE_WRITE`, reads that record (bounded, parsed
  strictly) only to tell its app whose remote it is, does nothing, and its app
  says the phone remote is in use by another account. Ownership is the open
  handle, never the record: a record left by a helper that died says nothing,
  since the next open succeeds. The lock file missing (not installed by this installer): the
  helper does nothing and says Glance needs reinstalling. The owner keeps the
  remote while its account's session is switched away (the phone keeps
  controlling that account's session) and gives it up when its helper exits
  (remote turned off, sign-out). The firewall rule and port belong to the
  owner; turning the remote off from another account changes only that
  account's setting.

### Installing, updating, uninstalling: maintenance

**No named kernel objects.** Any process may create a mutex or event in the
`Global\` namespace first (only global file mappings need
`SeCreateGlobalPrivilege`), and an event or mutex vanishes with its last
handle. Maintenance therefore rests on files in a folder only administrators
can write:

- `%ProgramData%\Glance\` (made by the installer): owner Administrators;
  SYSTEM and Administrators full; Authenticated Users read and traverse. The
  installer refuses to go on if the folder exists with another owner or a DACL
  granting users more (a same-user process cannot make it so: `%ProgramData%`
  lets users create folders, so if one already exists with the user as owner,
  the installer takes ownership, resets its DACL and empties it, all before
  use).
- `install.lock` in it: SYSTEM and Administrators only. The installer opens
  it with no sharing for its whole run; a second installer's open fails, and
  it waits up to 60 s, then refuses. No user process can open it, so none can
  hold it.
- `maintenance.json` in it, Authenticated Users read: `{ phase, from, to,
  started, stage_ok: [...] }`, ≤ 64 KiB, replaced atomically (temporary file
  in the folder, flushed, `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`),
  parsed bounded against a strict schema. Task snapshots are separate files
  in `%ProgramData%\Glance\snapshot\` (administrators only), named in the
  record; the record never grows with them.

**Admission.** Every start of the app, the service and the helper (first
starts, restarts, demand starts, each task's action), and the app again
before each start of the service and each elevation it asks for: take its own
single-instance object, then read the record. `done`, or no folder at all
(never installed by a maintenance-aware installer, such as a build run by
hand): go on. Any other phase, or a record unreadable or malformed: exit (the
app first shows its repair prompt, below). **While running**, the app, the
service and the helper read the record every second (it is tiny and
cached by the file system) and exit within a second of seeing a phase other
than `done`. Because a process exists before it reads the record, the
installer's handle-based wait (below) covers every process that may have read
an older one.

**Installing or updating** (the installer, elevated). A phase is written as
its step begins; a step's completion is written as it ends
(`stage_ok`), so recovery never trusts a step that did not finish.

1. Open `install.lock`. **Recover first** if the record is not `done`
   (below).
2. `stopping`: snapshot every task of this installation (definition XML,
   security descriptor, enabled state) and the helper's firewall rule into
   the snapshot folder; disable the tasks; write the phase, so that running
   processes leave on their own. Then **quiescence**: open a handle
   (`SYNCHRONIZE | PROCESS_TERMINATE`) to every process whose image is in the
   install folder; wait until none of the tasks has a running instance
   (`IRegisteredTask::GetInstances`) and every handle is signalled, ending
   (`TerminateProcess`) those not gone in 10 s and waiting for their handles;
   then scan again, twice 1 s apart, with the same treatment for anything
   new. If any process cannot be opened, ended, or seen gone within 30 s in
   all: **abort**: restore the tasks' enabled states, write `done` at the old
   version, report. `stage_ok: stopped`.
3. `staging`: copy the new files to `<install>.new` (the protected ACL, the
   existing reparse-point checks), verify each file's signature and version.
   `stage_ok: staged`.
4. `swapping`: rename `<install>` → `<install>.old`, then `<install>.new` →
   `<install>`. A rename failing (a file in use: someone started an old
   `glance.exe` by hand meanwhile) → quiescence again and one retry; failing
   again → roll back (below). `stage_ok: swapped`.
5. `tasks`: register the migrated tasks (below), **disabled**, each read back
   and compared. `stage_ok: tasks`.
6. `verifying`: the installed files' versions equal the new version and their
   signatures pass; the tasks equal their definitions; the folder, lock and
   lock-file ACLs are as specified. `stage_ok: verified`.
7. `committing`: enable each task as its snapshot had it, delete
   `<install>.old` and the snapshot files, write `done` at the new version,
   close `install.lock`, start the app as the user through the shell.

**Recovery** (from the record, before any new transaction): first
quiescence (step 2's, without the snapshot), then by the last completed step:

| Last completed | Folders found | Action |
| --- | --- | --- |
| none, `stopped` or `staged` | `<install>` (old), maybe `.new` | delete `.new`; restore tasks (definitions, descriptors, states) and the firewall rule from the snapshot; `done` at the old version |
| during `swapping` | `.old` and `.new`, no `<install>` | rename `.old` back; as above |
| during `swapping` | `.old` and `<install>` (new), no `.new` | roll forward from step 5 if `<install>` verifies, else swap back and roll back as above |
| `swapped` or `tasks` | `.old` and `<install>` (new) | roll forward from step 5 (or 6) |
| `verified` | as above | roll forward from step 7 |

The app's **repair prompt** (a stale or unreadable record): one dialog naming
the problem, with one button running `%ProgramData%\Glance\Glance-setup.exe
/repair` (kept there, administrators only, by every install), which asks for
elevation itself.

**Uninstalling**: steps 1–2, then `uninstalling`: delete the tasks, the
firewall rule, the files, the lock file of the helper; then the record and the
folder last. A failure leaves `uninstalling`, so nothing starts from the
remains; running the uninstaller again continues.

**Downgrades** are not supported: the installer refuses an older version than
the installed one; an installer of ≤ 0.3.7 run over a newer Glance is not a
supported way back (it would leave the newer tasks); going back is
uninstalling, then installing the older release.

**Now**: the installer kills by image name, copies over the running folder,
and leaves the tasks enabled throughout.

### Migration

Two sets of tasks are snapshotted in step 2, with their enabled states:
this installation's valid tasks (known names, action this install's
`glance.exe` or `glance-remote.exe`, principal the SID in the name) and legacy
tasks to repair (the launch task without arguments of ≤ 0.3.7; the 0.1.x
global names `Glance`, `Glance at sign-in`, `Glance Remote`; known names with
another principal), each only where its action is this install's executable.
Each is replaced by the current definition carrying its enabled state
(registered disabled in step 5, enabled in step 7); an old one is deleted only
after its replacement reads back equal. A launch task existing never turns
autostart on. Tasks whose action is elsewhere are not touched.

### Compatibility

| Meeting | What happens |
| --- | --- |
| A ≤ 0.3.7 Glance running when an update starts | ended in step 2's quiescence (its tasks disabled first) |
| A ≤ 0.3.7 `glance.exe` started by hand during an update | if it runs from the install folder, the swap fails and is retried once after quiescence, else rolled back and reported; after the swap, no old executable remains in the install folder |
| A ≤ 0.3.7 copy elsewhere (a second installation) | unsupported; the new app also checks the old single-instance name (`Local\Glance.SingleInstance`) and refuses to run beside it, saying another Glance is running |
| An app and a service of different versions | the bootstrap's package versions differ: retire, then the app starts its own version's service |
| An app and a helper of different versions | the shared memory gets a versioned header (`magic GLNR, u16 major, minor, patch, u32 count, u32 length`), changed in both programs together; a helper reading another version (or the old header) does nothing |
| An installer of ≤ 0.3.7 run over this version | it predates these rules and cannot be refused by them: unsupported, as stated above |

## Without the service

Declined, five deaths, a standard account, or another account's credentials:

- Readings: the app's own; the privileged ones shown as needing administrator
  rights, with a button that asks once.
- Input: the app's broker; over an elevated window pushes and button releases
  go unheard, resting on an edge still opens the panel, drags end on their own
  window's messages.
- Autostart: the sign-in task works; the service at sign-in does not.
- Updates: the app downloads the installer to `%LOCALAPPDATA%\Glance\update`,
  verifies it as the service would, and runs it; Windows' elevation prompt
  shows the verified publisher of the file actually run, and the user's
  consent there is the binding. **Now** this path fails writing a protected
  folder.
- The phone remote's switch: `runas` of `glance.exe --remote on|off`, a command
  doing only that, with the same checks as the service's `Switch`.

## Threads in the app

- Main thread: the tray window and its menu.
- Panel thread: the panel, widgets, overlay, settings window, menus, tips,
  dialogs, lyric windows, the drag controller. It only enqueues work for the
  link, network, reports and switches, and is told back by posted messages.
  **Now** the settings' switches wait up to 20 s on it and reports are put
  together on it.
- Workers: the link's reader and writer, media, update checks, the widgets'
  window watch (four looks a second), reports.
- No WinEvent hooks anywhere in the app: in the Task Manager hang the panel
  thread, hooked out of context for most event types, did nothing but take
  those callbacks for over a minute (trace of 2026-10-12); polling replaced it.

## Verification

### Conditions

| Condition | How |
| --- | --- |
| Elevated window in front | an elevated plain window (`elevwin.ps1`); isolates UIPI |
| Task Manager's event flood | opened, with and without TRCC; the panel thread answers within 100 ms throughout |
| No service | declined; killed; standard account; another account's credentials |
| Updates | from 0.1.x (global tasks), 0.3.6, 0.3.7 (whole-app elevated task); installer killed in each phase; two installers; a ≤ 0.3.7 task firing during maintenance; a disabled task kept disabled |
| Uninstall | app, service and helper running |
| Fast user switching | two accounts in the VM, both with Glance; each its own service; the phone remote owned by one, refused in the other |
| Same account twice | not producible on Windows 11 client; the single-instance object's name is checked in a unit test |
| Hardware | the VM for logic and safety; the owner's PC for PawnIO, ASUS WMI, NVMe/SATA, TRCC, a real mouse |

### Input cases (with and without an elevated window in front)

Open by a push and by resting; drag, resize and tear off the panel; release
during the tear-off before the widget shows; a delayed release of an earlier
press arriving after a new press; release outside every window; capture taken
mid-drag; hide, destroy, lock mid-drag; the service killed mid-drag; swapped
buttons, and swapping during a press; volume in the panel and in a widget;
widgets and the overlay; the lyric windows' move and size; pen and touch; a
DPI change mid-drag. Each asserts the final geometry or volume in screen
coordinates, no drag left held, no click or fling on a cancel, no Glance window
activated.

### Link and lifecycle cases

A squatted pipe name; a client of the wrong image, SID, session or
integrity; a medium process trying `WRITE_DAC` and a second instance; a
rejected client reconnecting in a loop; a frame header stalled after one
byte; partial, oversized and malformed frames; a reader that stops; a flood of
requests; late and duplicate replies; a cancellation racing a completion;
protocol and build-kind mismatch; a hung sampler, worker and download;
watchdog exits; restarts versus five deaths; the maintenance and quit objects
pre-made by another process; a stale maintenance record.

### Budgets

The panel thread answers within 100 ms; link queues ≤ 1 MiB a side; a first
restart within 3 s of a death (later ones by the backoff); a hung service
dropped by the app within 5 s and ended by its watchdog within the times of the
table in "Liveness" (a stall the watchdog cannot see, its own, is not
bounded: the app works without the service meanwhile); requests answered by
their deadlines.

## Stages

Each stage ships whole in one installer; the app and service always come from
the same version (the bootstrap handles a service left from the previous one).

1. **Boundary and lifecycle**: no elevated UI on any start path; the pipe,
   verification, bootstrap, frames, queues, requests, liveness and watchdog;
   the readings split and `Readings`; the service's log through the app; the
   capabilities with their bounds, the update signer and no-downgrade; the
   task definitions and descriptors; cross-account refusal; maintenance,
   migration, the per-account names; the helper's machine-wide owner; the
   settings' switches and reports off the panel thread. Input as now (known
   limits over elevated windows).
   Compatibility and rollback: as in "Compatibility" (the installer's
   stopping phase and admission keep old and new from running together; a
   service left from another version is retired by the bootstrap; downgrades
   go through uninstalling).
2. **Input** (built; see "As built"): the broker (edges, buttons, activity)
   and its messages; drags ended by their windows' own messages; dismissal by
   the service's buttons; no global input read in the linked app. Owed: the
   owner's real-mouse check.

## Answers to the third review

Blockers:

1. Maintenance: an admission protocol for every start (and before each
   service start and elevation the app asks for); the record's folder and file
   ACLs, atomic replacement, bounded parsing, fail-closed outcomes; recovery
   before a new transaction, phase by phase, with full task definitions and
   descriptors; `done` only after verification; failed uninstall keeps starts
   suppressed; ≤ 0.3.7 stopped by disabling tasks and waiting for their
   instances and processes; a mutex users cannot open, a manual-reset quit
   event users can only wait on, both validated when they already exist; the
   one repair prompt.
2. Compatibility: a matrix in place of "cannot happen"; package versions in
   the bootstrap; downgrades and ≤ 0.3.7 installers declared unsupported, the
   way back being uninstall then install; two task sets (valid and legacy to
   repair), both snapshotted.
3. The helper's owner: an installer-made lock file a medium helper can hold
   open exclusively, machine-wide, recording its holder; behaviour on a
   switched-away session and on exit.
4. Updates: the service reads the installer's own version (equal to the
   request, above the installed one); the root matched by its SHA-256; the
   profile OID read on the signer WinVerifyTrust accepted; the revocation
   policy; the bytes kept open against change until the installer exits; no
   lock held by the service, the installer's check under its lock authoritative.

Settled in the draft too: the task's owner, `TASK_DONT_ADD_PRINCIPAL_ACE`,
read-back, full definition, names kept as now (5); the shell hand-off through
the desktop's own automation object, the shell's identity checked, the
`--handed-off` marker (6); `OpenThreadToken(…, TRUE)` (7); overall deadlines
and timeout outcomes, read and connect cancellation under the watchdog, loop
progress, `QueryUnbiasedInterruptTime`, a budget table, per-group acquisition
times (8); control places that cannot fill in a healthy connection and close
it if they do, `Edges` within its frame, ids strictly increasing, `Resync`
sent at once (9). Stage 2: the drag controller's latch, reentrancy checks,
`GetCapture` after the transfer, stale messages, association by physical
button and press mapping, uncertain association cancels, the 10 s bound (10);
activity in the helper's existing format, cleared on loss (11). Facts:
`may_start_unasked` described as it is; UAC off by account kind.
