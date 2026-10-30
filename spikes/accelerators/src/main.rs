//! Which execution providers the Windows ML catalog offers on this machine, what getting them
//! ready costs, and which devices ONNX Runtime then sees (docs/M7_ACCELERATORS.md).
//!
//! accelerators-spike catalog <winml-dir>            list the catalog
//! accelerators-spike ensure <winml-dir> <name>      download / make ready one provider
//! accelerators-spike devices <winml-dir>            register every ready provider, list devices

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};

#[repr(C)]
struct EpInfo {
    name: *const c_char,
    version: *const c_char,
    package_family_name: *const c_char,
    library_path: *const c_char,
    package_root_path: *const c_char,
    ready_state: i32,
    certification: i32,
}

type Handle = *mut c_void;
type EnumCallback = unsafe extern "system" fn(Handle, *const EpInfo, *mut c_void) -> i32;

struct Catalog {
    _lib: libloading::Library,
    handle: Handle,
    enumerate: unsafe extern "system" fn(Handle, EnumCallback, *mut c_void) -> i32,
    find: unsafe extern "system" fn(Handle, *const c_char, *const c_char, *mut Handle) -> i32,
    ensure: unsafe extern "system" fn(Handle) -> i32,
}

#[derive(Debug, Clone)]
struct Provider {
    name: String,
    version: String,
    package: String,
    library: String,
    ready: i32,
    certified: i32,
}

fn text(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

unsafe extern "system" fn collect(_: Handle, info: *const EpInfo, context: *mut c_void) -> i32 {
    let list = unsafe { &mut *(context as *mut Vec<Provider>) };
    let info = unsafe { &*info };
    list.push(Provider {
        name: text(info.name),
        version: text(info.version),
        package: text(info.package_family_name),
        library: text(info.library_path),
        ready: info.ready_state,
        certified: info.certification,
    });
    1
}

impl Catalog {
    fn open(dir: &Path) -> Result<Self> {
        unsafe {
            let lib = libloading::Library::new(dir.join("Microsoft.Windows.AI.MachineLearning.dll"))?;
            let create: libloading::Symbol<unsafe extern "system" fn(*mut Handle) -> i32> =
                lib.get(b"WinMLEpCatalogCreate")?;
            let mut handle = std::ptr::null_mut();
            let hr = create(&mut handle);
            if hr < 0 {
                bail!("WinMLEpCatalogCreate failed: {hr:#x}");
            }
            let enumerate = *lib.get(b"WinMLEpCatalogEnumProviders")?;
            let find = *lib.get(b"WinMLEpCatalogFindProvider")?;
            let ensure = *lib.get(b"WinMLEpEnsureReady")?;
            Ok(Self { _lib: lib, handle, enumerate, find, ensure })
        }
    }

    fn providers(&self) -> Result<Vec<Provider>> {
        let mut list: Vec<Provider> = Vec::new();
        let hr = unsafe { (self.enumerate)(self.handle, collect, &mut list as *mut _ as *mut c_void) };
        if hr < 0 {
            bail!("WinMLEpCatalogEnumProviders failed: {hr:#x}");
        }
        Ok(list)
    }

    fn ensure(&self, name: &str) -> Result<()> {
        let name = std::ffi::CString::new(name)?;
        let mut ep = std::ptr::null_mut();
        let hr = unsafe { (self.find)(self.handle, name.as_ptr(), std::ptr::null(), &mut ep) };
        if hr < 0 {
            bail!("WinMLEpCatalogFindProvider failed: {hr:#x}");
        }
        let hr = unsafe { (self.ensure)(ep) };
        if hr < 0 {
            bail!("WinMLEpEnsureReady failed: {hr:#x}");
        }
        Ok(())
    }
}

const STATES: [&str; 3] = ["ready", "not ready", "not present"];
const CERTS: [&str; 3] = ["unknown", "certified", "uncertified"];

fn print(list: &[Provider]) {
    for p in list {
        println!(
            "{} {} [{}; {}] package={} library={}",
            p.name,
            p.version,
            STATES.get(p.ready as usize).unwrap_or(&"?"),
            CERTS.get(p.certified as usize).unwrap_or(&"?"),
            p.package,
            p.library
        );
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(args.get(1).context("winml dir")?);
    let catalog = Catalog::open(&dir)?;
    match args[0].as_str() {
        "catalog" => print(&catalog.providers()?),
        "ensure" => {
            let name = args.get(2).context("provider name")?;
            let start = Instant::now();
            catalog.ensure(name)?;
            println!("{name} ready after {:.1} s", start.elapsed().as_secs_f64());
            print(&catalog.providers()?);
        }
        "devices" => {
            unsafe { std::env::set_var("ORT_DISABLE_TELEMETRY", "1") };
            let env = ort::init_from(dir.join("onnxruntime.dll"))?
                .with_name("spike")
                .with_telemetry(false)
                .build()?;
            for p in catalog.providers()? {
                if p.ready == 0 && !p.library.is_empty() {
                    match env.register_ep_library(&p.name, &p.library) {
                        Ok(()) => println!("registered {}", p.name),
                        Err(e) => println!("could not register {}: {e}", p.name),
                    }
                }
            }
            for d in env.devices() {
                let h = d.hardware_device();
                println!(
                    "{} | {} {:?} id={}",
                    d.ep()?,
                    h.vendor()?,
                    h.ty(),
                    h.id()
                );
            }
        }
        other => bail!("unknown command {other}"),
    }
    Ok(())
}
