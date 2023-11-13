// 注意例子要看对应版本的，master上的编译不过
// https://github.com/hyperium/hyper/blob/0.14.x/examples/send_file.rs
// https://github.com/hyperium/hyper/blob/0.14.x/examples/single_threaded.rs

use arc_swap::ArcSwap;
use dashmap::DashMap;
use emacs::{defun, Vector};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Request, Response, Result, Server, StatusCode};
use once_cell::sync::Lazy;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Barrier,
};
use std::thread::JoinHandle;
use tokio::fs::File;
use tokio::sync::oneshot;
use tokio_util::codec::{BytesCodec, FramedRead};

struct Web {
    handle: Option<(JoinHandle<()>, oneshot::Sender<()>)>, // 线程join和信号send都需要self
    web_context: Arc<WebContext>,
}
struct WebContext {
    web_root: ArcSwap<Vec<String>>,   // ArcSwap<String>代替Mutex<String>
    content: DashMap<String, String>, // DashMap<path, content>，代替Mutex<HashMap<...>>
}
impl WebContext {
    fn get_content(&self, path: &String) -> Option<String> {
        if let Some(ref r) = self.content.get(path) {
            return Some(r.value().clone());
        }
        None
    }
}
static GLOBAL_WEB_COOKIE: AtomicUsize = AtomicUsize::new(1);
static GLOBAL_WEBS: Lazy<DashMap<usize, Web>> = Lazy::new(|| DashMap::new());

fn to_wstring(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn debug_msg(msg: &str) {
    use winapi::um::debugapi::OutputDebugStringW;
    unsafe {
        OutputDebugStringW(to_wstring(&msg).as_slice().as_ptr());
    }
}

/// HTTP status code 404
fn not_found(filename: String) -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(filename.into())
        .unwrap()
}
async fn simple_file_send(root: &Vec<String>, filename: String) -> Result<Response<Body>> {
    for r in root {
        let mut filename = filename.clone();
        filename.insert_str(0, r.as_str());
        // debug_msg(&format!("send file: {}", filename));
        if let Ok(file) = File::open(&filename).await {
            let stream = FramedRead::new(file, BytesCodec::new());
            let body = Body::wrap_stream(stream);
            return Ok(Response::new(body));
        }
    }
    Ok(not_found(filename))
}

async fn response_examples(req: Request<Body>, context: Arc<WebContext>) -> Result<Response<Body>> {
    use percent_encoding::percent_decode_str;
    let path: String = percent_decode_str(req.uri().path())
        .decode_utf8_lossy()
        .into_owned();
    if let Some(c) = (*context).get_content(&path) {
        Ok(Response::new(c.into()))
    } else {
        let root = (*context).web_root.load();
        simple_file_send(&**root, path).await
    }
}
// (ignore-errors (module-load "H:/prj/rust/emacs-preview-rs/target/release/emacs_preview_rs.dll"))
// (ignore-errors (module-load "f:/prj/rust/emacs-preview-rs/target/release/emacs_preview_rs.dll"))
// (emacs-preview-rs/web-server-start "127.0.0.1" 1888)
// (emacs-preview-rs/web-server-set-root 2 "f:/prj/rust/emacs-preview-rs/src/")
// (emacs-preview-rs/web-server-stop 2)
// (emacs-preview-rs/web-server-set-content 2 "/" "<center>aaa</center>")
async fn run(
    web_context: Arc<WebContext>,
    host: String,
    port: u16,
    stop_sig: oneshot::Receiver<()>,
) {
    debug_msg(&format!("run: {}:{}", host, port));
    if let Ok(addr) = format!("{}:{}", host, port).parse() {
        let make_service = make_service_fn(|_| {
            let web_context = web_context.clone();
            async {
                Ok::<_, Error>(service_fn(move |req| {
                    let web_context = web_context.clone();
                    response_examples(req, web_context)
                }))
            }
        });

        let server = Server::bind(&addr).executor(LocalExec).serve(make_service);
        let server = server.with_graceful_shutdown(async move {
            stop_sig.await.ok();
        });
        debug_msg(&format!("Listening on http://{}", addr));
        if let Err(e) = server.await {
            debug_msg(&format!("server error:{}, {}:{}", e, host, port));
        }
    } else {
        debug_msg(&format!("failed parse addr: {}:{}", host, port));
    }
}

// Since the Server needs to spawn some background tasks, we needed
// to configure an Executor that can spawn !Send futures...
#[derive(Clone, Copy, Debug)]
struct LocalExec;

impl<F> hyper::rt::Executor<F> for LocalExec
where
    F: std::future::Future + 'static, // not requiring `Send`
{
    fn execute(&self, fut: F) {
        // This will spawn into the currently running `LocalSet`.
        tokio::task::spawn_local(fut);
    }
}

#[defun]
fn web_server_start(host: String, port: u16) -> emacs::Result<usize> {
    let (ss, sr) = oneshot::channel::<()>();
    let run_result = Arc::new(AtomicBool::new(false));
    let run_result2 = run_result.clone();
    let barrier = Arc::new(Barrier::new(2));
    let b2 = barrier.clone();

    let web_context = Arc::new(WebContext {
        content: DashMap::new(),
        web_root: ArcSwap::new(Arc::new(Vec::new())),
    });
    let web_context1 = web_context.clone();
    let tj = std::thread::spawn(move || {
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            run_result.fetch_or(true, Ordering::SeqCst);
            b2.wait();
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async {
                run(web_context, host, port, sr).await;
            });
        } else {
            debug_msg(&format!("failed to start tokio thread: {}:{}", host, port));
            b2.wait();
        }
    });
    barrier.wait();
    if !run_result2.load(Ordering::SeqCst) {
        return Ok(0);
    }

    let web = Web {
        handle: Some((tj, ss)),
        web_context: web_context1,
    };
    GLOBAL_WEB_COOKIE.fetch_add(1, Ordering::SeqCst);
    let web_index = GLOBAL_WEB_COOKIE.load(Ordering::SeqCst);
    GLOBAL_WEBS.insert(web_index, web);
    Ok(web_index)
}

#[defun]
fn web_server_stop(web_index: usize) -> emacs::Result<bool> {
    if let Some(ref mut web) = GLOBAL_WEBS.get_mut(&web_index) {
        if let Some((t, s)) = web.value_mut().handle.take() {
            s.send(()).ok(); // 测试不会马上结束，需要网页再访问一次才退出，不过问题不大，网页有js自动刷新请求
            t.join().ok();
        }
    }
    Ok(true)
}

#[defun]
fn web_server_set_root(web_index: usize, web_roots: Vector) -> emacs::Result<bool> {
    let mut v: Vec<String> = Vec::new();
    for i in 0..web_roots.len() {
        if let Ok(web_root) = web_roots.get::<String>(i) {
            let web_root = web_root.replace("\\", "/");
            let web_root = web_root.trim_end_matches('/');
            let web_root = web_root.to_string();
            v.push(web_root);
        }
    }
    if let Some(ref web) = GLOBAL_WEBS.get(&web_index) {
        // debug_msg(&format!("set root: {:?}", v));
        web.value().web_context.web_root.store(Arc::new(v));
    }
    Ok(true)
}

#[defun]
fn web_server_set_content(web_index: usize, path: String, content: String) -> emacs::Result<bool> {
    if let Some(ref web) = GLOBAL_WEBS.get(&web_index) {
        let mut path_index = path.clone();
        web.web_context.content.insert(path, content);

        // 创建一个_index用于记录内容是否变化的标记
        path_index += "_index";
        GLOBAL_WEB_COOKIE.fetch_add(1, Ordering::SeqCst);
        web.web_context.content.insert(
            path_index,
            format!("{}", GLOBAL_WEB_COOKIE.load(Ordering::SeqCst)),
        );
    }
    Ok(true)
}

// Emacs won't load the module without this.
emacs::plugin_is_GPL_compatible!();

#[emacs::module(separator = "/")]
fn init(_: &emacs::Env) -> emacs::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
