use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

const INTERRUPTED: i32 = 130;

static MESSAGE: AtomicPtr<Vec<u8>> = AtomicPtr::new(ptr::null_mut());
static CHILD: AtomicI32 = AtomicI32::new(0);
static REQUEST_FD: AtomicI32 = AtomicI32::new(-1);

pub fn on_interrupt(message: String) {
    let leaked: &'static mut Vec<u8> = Box::leak(Box::new(message.into_bytes()));
    MESSAGE.store(leaked, Ordering::SeqCst);
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle as extern "C" fn(libc::c_int) as libc::sighandler_t;
        action.sa_flags = libc::SA_RESETHAND;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGINT, &action, ptr::null_mut());
    }
}

pub fn watch_child(pid: i32, request_fd: i32) {
    REQUEST_FD.store(request_fd, Ordering::SeqCst);
    CHILD.store(pid, Ordering::SeqCst);
}

extern "C" fn handle(_: libc::c_int) {
    unsafe {
        if let Some(message) = MESSAGE.load(Ordering::SeqCst).as_ref() {
            libc::write(2, message.as_ptr().cast(), message.len());
        }
        let request_fd = REQUEST_FD.load(Ordering::SeqCst);
        if request_fd >= 0 {
            libc::close(request_fd);
        }
        let child = CHILD.load(Ordering::SeqCst);
        if child > 0 {
            let mut status = 0;
            libc::waitpid(child, &mut status, 0);
        }
        libc::_exit(INTERRUPTED);
    }
}
