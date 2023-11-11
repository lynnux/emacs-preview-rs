// 注意例子要看对应版本的，master上的编译不过
// https://github.com/hyperium/hyper/blob/0.14.x/examples/send_file.rs
// https://github.com/hyperium/hyper/blob/0.14.x/examples/single_threaded.rs

use emacs::defun;
use once_cell::sync::Lazy;
use std::thread::JoinHandle;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Barrier, Mutex,
    },
};
use tokio::sync::oneshot;
use tokio::fs::File;
use hyper::body::{Bytes, HttpBody};
use hyper::header::{HeaderMap, HeaderValue};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Error, Response, Server, Body, Method, StatusCode, Request, Result};
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_util::codec::{BytesCodec, FramedRead};

struct Web {
    handle: Option<(JoinHandle<()>, oneshot::Sender<()>)>, // 线程join和信号send都需要self
    web_context: Arc<WebContext>,
}
struct WebContext{
    web_index : usize,
    web_root: String,
    content: Mutex<(String, usize)>,
}
impl WebContext{
    fn get_content(&self) -> String{
        let mg = self.content.lock().unwrap();
        let (ref c, _) = *mg;
        c.clone()
    }
    fn get_content_index(&self) -> usize{
        let mg = self.content.lock().unwrap();
        let (_, i) = *mg;
        i
    }
}
static GLOBAL_WEB_COOKIE: AtomicUsize = AtomicUsize::new(1);
static GLOBAL_WEBS: Lazy<Mutex<HashMap<usize, Web>>> = Lazy::new(|| Mutex::new(HashMap::new()));

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
async fn simple_file_send(filename: String) -> Result<Response<Body>> {
    if let Ok(file) = File::open(&filename).await {
        let stream = FramedRead::new(file, BytesCodec::new());
        let body = Body::wrap_stream(stream);
        return Ok(Response::new(body));
    }
    Ok(not_found(filename))
}

async fn response_examples(req: Request<Body>, context: Arc<WebContext>) -> Result<Response<Body>> {
    match (req.uri().path()){
        "/get_content" => Ok(Response::new((*context).get_content().into())),
        "/get_content_index" => Ok(Response::new(format!("{}", (*context).get_content_index()).into())),
        "/" => simple_file_send((*context).web_root.clone() + "/index.html").await,
        _  => simple_file_send((*context).web_root.clone() + req.uri().path()).await,
    }
}
// (ignore-errors (module-load "H:/prj/rust/emacs-preview-rs/target/release/emacs_preview_rs.dll"))
// (ignore-errors (module-load "f:/prj/rust/emacs-preview-rs/target/release/emacs_preview_rs.dll"))
// (emacs-preview-rs/web-server-start "f:/prj/rust/emacs-preview-rs/src/" "127.0.0.1" 1888)
// (emacs-preview-rs/web-server-stop 4)
// (emacs-preview-rs/web-server-set-content 2 "aaa")
async fn run(web_context : Arc<WebContext>, host: String, port: u16, stop_sig: oneshot::Receiver<()>) {
    debug_msg(&format!("run: {}:{} at {}", host, port, web_context.web_root));
    if let Ok(addr) = format!("{}:{}", host, port).parse() {
        let make_service = make_service_fn(|_|{
            let web_context = web_context.clone();
            async  {
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
        debug_msg(&format!("Listening on http://{} at {}", addr, web_context.web_root));
        if let Err(e) = server.await {
            debug_msg(&format!(
                "server error:{}, {}:{} at {}",
                e, host, port, web_context.web_root
            ));
        }
    } else {
        debug_msg(&format!(
            "failed parse addr: {}:{} at {}",
            host, port, web_context.web_root
        ));
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
fn web_server_start(web_root: String, host: String, port: u16) -> emacs::Result<usize> {
    let (ss, sr) = oneshot::channel::<()>();
    let run_result = Arc::new(AtomicBool::new(false));
    let run_result2 = run_result.clone();
    let barrier = Arc::new(Barrier::new(2));
    let b2 = barrier.clone();
    GLOBAL_WEB_COOKIE.fetch_add(1, Ordering::SeqCst);
    let web_index = GLOBAL_WEB_COOKIE.load(Ordering::SeqCst);
    
    let web_root = web_root.replace("\\", "/");
    let web_root = web_root.trim_right_matches('/');
    let web_root = web_root.to_string();
    let web_context = Arc::new(WebContext{content: Mutex::new(("".into(), 0)) , web_index, web_root});
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
            debug_msg(&format!(
                "failed to start tokio thread: {}:{} at {}",
                host, port, web_context.web_root
            ));
            b2.wait();
        }
    });
    barrier.wait();
    if !run_result2.load(Ordering::SeqCst) {
        return Ok(0);
    }

    let web = Web {
        handle: Some((tj, ss)),
        web_context: web_context1
    };

    GLOBAL_WEBS.lock().unwrap().insert(web_index, web);
    Ok(web_index)
}

#[defun]
fn web_server_stop(web_index: usize) -> emacs::Result<bool> {
    let mut webs = GLOBAL_WEBS.lock().unwrap();
    if let Some(ref mut web) = webs.get_mut(&web_index) {
        if let Some((t, s)) = web.handle.take() {
            s.send(()).ok(); // 测试不会马上结束，需要网页再访问一次才退出，不过问题不大，网页有js自动刷新请求
            t.join().ok();
        }
    }
    Ok(true)
}

#[defun]
fn web_server_set_content(web_index: usize, content: String) -> emacs::Result<usize> {
    let mut webs = GLOBAL_WEBS.lock().unwrap();
    if let Some(ref web) = webs.get(&web_index) {
        let mut mg = web.web_context.content.lock().unwrap();
        let (ref mut c, ref mut i) = *mg;
        *c = content;
        *i = *i + 1;
    }
    Ok(0)
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
