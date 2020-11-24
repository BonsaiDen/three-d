use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use log::info;
use std::path::Path;

#[cfg(feature = "3d-io")]
pub mod threed;

#[cfg(feature = "3d-io")]
pub use threed::*;

#[cfg(feature = "obj-io")]
pub mod obj;

#[cfg(feature = "obj-io")]
pub use obj::*;


#[derive(Debug)]
pub enum Error {
    #[cfg(feature = "image-io")]
    Image(image::ImageError),
    #[cfg(feature = "3d-io")]
    Bincode(bincode::Error),
    #[cfg(feature = "obj-io")]
    Obj(wavefront_obj::ParseError),
    #[cfg(not(target_arch = "wasm32"))]
    IO(std::io::Error),
    FailedToLoad {message: String}
}

#[cfg(feature = "image-io")]
impl From<image::ImageError> for Error {
    fn from(other: image::ImageError) -> Self {
        Error::Image(other)
    }
}

#[cfg(feature = "3d-io")]
impl From<bincode::Error> for Error {
    fn from(other: bincode::Error) -> Self {
        Error::Bincode(other)
    }
}

#[cfg(feature = "obj-io")]
impl From<wavefront_obj::ParseError> for Error {
    fn from(other: wavefront_obj::ParseError) -> Self {
        Error::Obj(other)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<std::io::Error> for Error {
    fn from(other: std::io::Error) -> Self {
        Error::IO(other)
    }
}

pub type Loaded = HashMap<String, Result<Vec<u8>, std::io::Error>>;
type RefLoaded = Rc<RefCell<Loaded>>;

pub struct Loader {
}

impl Loader {

    pub fn load<F>(paths: &[&'static str], on_done: F)
        where F: 'static + FnOnce(&mut Loaded)
    {
        Self::load_with_progress(paths, |progress| {
                    info!("Progress: {}%", 100.0f32 * progress);
        }, on_done);
    }

    pub fn load_with_progress<F, G>(paths: &[&'static str], progress_callback: G, on_done: F)
        where
            G: 'static + Fn(f32),
            F: 'static + FnOnce(&mut Loaded)
    {
        let loads = Rc::new(RefCell::new(HashMap::new()));
        for path in paths {
            loads.borrow_mut().insert((*path).to_owned(), Ok(Vec::new()));
            Self::load_file(*path,loads.clone());
        }
        info!("Loading started...");
        Self::wait_local(loads.clone(), progress_callback, on_done);
    }

    pub fn get<'a>(loaded: &'a Loaded, path: &'a str) -> Result<&'a [u8], Error> {
        let bytes = loaded.get(&path.to_string()).ok_or(
            Error::FailedToLoad {message:format!("Tried to use a resource which was not loaded: {}", path)})?.as_ref()
            .map_err(|_| Error::FailedToLoad {message:format!("Could not load resource: {}", path)})?;
        Ok(bytes)
    }

    #[cfg(feature = "image-io")]
    pub fn get_image<'a>(loaded: &'a Loaded, path: &'a str) -> Result<image::DynamicImage, Error> {
        let img = image::load_from_memory(Self::get(loaded, path)?)?;
        Ok(img)
    }

    fn wait_local<F, G>(loads: RefLoaded, progress_callback: G, on_done: F)
        where
            G: 'static + Fn(f32),
            F: 'static + FnOnce(&mut Loaded)
    {
        Self::sleep(100, move || {

            let is_loading = match loads.try_borrow() {
                Ok(map) => {
                    let total_count = map.len();
                    let mut count = 0;
                    for bytes in map.values() {
                        if bytes.is_err() || bytes.as_ref().unwrap().len() > 0 {
                            count = count + 1;
                        }
                    }
                    progress_callback(count as f32 / total_count as f32);
                    count < total_count
                },
                Err(_) => true
            };

            if is_loading {
                Self::wait_local(loads, progress_callback, on_done);
            } else {
                info!("Loading done.");
                on_done(&mut loads.borrow_mut());
            }
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sleep<F>(millis: u64, fun: F)
    where
        F: 'static + FnOnce()
    {
        std::thread::sleep(std::time::Duration::from_millis(millis));
        fun();
    }

    #[cfg(target_arch = "wasm32")]
    fn sleep<F>(millis: u64, fun: F)
    where
        F: 'static + FnOnce()
    {
        use gloo_timers::callback::Timeout;
        let timeout = Timeout::new(millis as u32, fun);
        timeout.forget();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_file(path: &'static str, loads: RefLoaded)
    {
        let file = std::fs::File::open(path);
        match file {
            Ok(mut f) => {
                use std::io::prelude::*;
                let mut bytes = Vec::new();
                let result = f.read_to_end(&mut bytes).and(Ok(bytes));
                loads.borrow_mut().insert(path.to_owned(), result);
            },
            Err(e) => {loads.borrow_mut().insert(path.to_owned(), Err(e));}
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn load_file(path: &'static str, loads: RefLoaded)
    {
        wasm_bindgen_futures::spawn_local(Self::load_file_async(path, loads));
    }

    #[cfg(target_arch = "wasm32")]
    async fn load_file_async(url: &'static str, loads: RefLoaded)
    {
        use wasm_bindgen::prelude::*;
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode, Response};

        let mut opts = RequestInit::new();
        opts.method("GET");
        opts.mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(url, &opts).unwrap();
        request.headers().set("Accept", "application/octet-stream").unwrap();

        let window = web_sys::window().unwrap();
        let resp_value = JsFuture::from(window.fetch_with_request(&request)).await.unwrap();
        let resp: Response = resp_value.dyn_into().unwrap();

        // Convert this other `Promise` into a rust `Future`.
        let data: JsValue = JsFuture::from(resp.array_buffer().unwrap()).await.unwrap();
        loads.borrow_mut().insert(url.to_owned(), Ok(js_sys::Uint8Array::new(&data).to_vec()));
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Saver {

}

#[cfg(not(target_arch = "wasm32"))]
impl Saver {
    pub fn save_3d_file<P: AsRef<Path>>(path: P, cpu_meshes: &Vec<crate::CPUMesh>) -> Result<(), Error>
    {
        let mut input = Vec::new();
        let mut i = 0;
        for cpu_mesh in cpu_meshes {
            let texture_path = if let Some(ref img) = cpu_mesh.texture {
                let tex_path = path.as_ref().parent().unwrap().join(format!("{}{}", i, ".png"));
                i += 1;
                img.save_with_format(&tex_path, image::ImageFormat::Png)?;
                Some(tex_path)
            } else {None};
            input.push((cpu_mesh, texture_path));
        }

        let bytes = ThreeD::serialize(&input)?;
        if path.as_ref().ends_with(".3d") {
            Self::save_file(path, &bytes)?;
        } else {
            Self::save_file(path.as_ref().join(".3d"), &bytes)?;
        }
        Ok(())
    }

    pub fn save_file<P: AsRef<Path>>(path: P, bytes: &[u8]) -> Result<(), Error>
    {
        let mut file = std::fs::File::create(path)?;
        use std::io::prelude::*;
        file.write_all(bytes)?;
        Ok(())
    }
}
