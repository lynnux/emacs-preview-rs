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

use hyper::body::{Bytes, HttpBody};
use hyper::header::{HeaderMap, HeaderValue};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Error, Response, Server};
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

struct Web {
    handle: Option<(JoinHandle<()>, oneshot::Sender<()>)>, // 线程join和信号send都需要self
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

struct Body {
    // Our Body type is !Send and !Sync:
    _marker: PhantomData<*const ()>,
    data: Option<Bytes>,
}

impl From<String> for Body {
    fn from(a: String) -> Self {
        Body {
            _marker: PhantomData,
            data: Some(a.into()),
        }
    }
}

impl HttpBody for Body {
    type Data = Bytes;
    type Error = Error;

    fn poll_data(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        Poll::Ready(self.get_mut().data.take().map(Ok))
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
}

async fn run(web_root: String, host: String, port: u16, stop_sig: oneshot::Receiver<()>) {
    debug_msg(&format!("run: {}:{} at {}", host, port, web_root));
    if let Ok(addr) = format!("{}:{}", host, port).parse() {
        let make_service = make_service_fn(move |_| async move {
            Ok::<_, Error>(service_fn(move |_| async move {
                Ok::<_, Error>(Response::new(Body::from(format!("Request #{}", 1))))
            }))
        });

        let server = Server::bind(&addr).executor(LocalExec).serve(make_service);
        let server = server.with_graceful_shutdown(async move {
            stop_sig.await.ok();
        });
        debug_msg(&format!("Listening on http://{} at {}", addr, web_root));
        if let Err(e) = server.await {
            debug_msg(&format!(
                "server error:{}, {}:{} at {}",
                e, host, port, web_root
            ));
        }
    } else {
        debug_msg(&format!(
            "failed parse addr: {}:{} at {}",
            host, port, web_root
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
    let tj = std::thread::spawn(move || {
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            run_result.fetch_or(true, Ordering::SeqCst);
            b2.wait();
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async {
                run(web_root, host, port, sr).await;
            });
        } else {
            debug_msg(&format!(
                "failed to start tokio thread: {}:{} at {}",
                host, port, web_root
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
    };
    GLOBAL_WEB_COOKIE.fetch_add(1, Ordering::SeqCst);
    let now = GLOBAL_WEB_COOKIE.load(Ordering::SeqCst);
    GLOBAL_WEBS.lock().unwrap().insert(now, web);
    Ok(now)
}

#[defun]
fn web_server_stop(web_handle: usize) -> emacs::Result<bool> {
    let mut webs = GLOBAL_WEBS.lock().unwrap();
    if let Some(ref mut web) = webs.get_mut(&web_handle) {
        if let Some((t, s)) = web.handle.take() {
            t.join().ok();
            s.send(()).ok();
        }
    }
    Ok(true)
}

#[defun]
fn web_server_set_content(_web_handle: usize) -> emacs::Result<usize> {
    Ok(0)
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
