use std::{os::fd::AsRawFd, pin::Pin, sync::Arc, task::Poll};

use io_uring::{IoUring, types};
use parking_lot::Mutex;
use pin_project::pin_project;

pub trait AsIoVec: AsRef<[u8]> {
    fn as_io_vec(&self) -> (*mut u8, usize);
}

impl<A: ?Sized + AsRef<[u8]>> AsIoVec for A {
    fn as_io_vec(&self) -> (*mut u8, usize) {
        (self.as_ref().as_ptr() as *mut u8, self.as_ref().len())
    }
}

pub trait AsIoVecMut: AsMut<[u8]> {}

impl<A: ?Sized + AsIoVec + AsMut<[u8]>> AsIoVecMut for A {}

pub struct Ring {
    inner: Arc<Mutex<IoUring>>,
}

impl Ring {
    pub fn new(entires: u32) -> std::io::Result<Self> {
        let ring = IoUring::new(entires)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(ring)),
        })
    }

    pub fn read_at<'a, F: AsRawFd, B: AsIoVec + AsIoVecMut>(
        &'a self,
        fd: &'a F,
        buf: &'a mut B,
        offset: u64,
    ) -> ReadComplection<'a, F, B> {
        ReadComplection::new(self, fd, buf, offset)
    }

    fn submit(&self) -> std::io::Result<()> {
        let mut inner = self.inner.lock();
        inner.submit()?;

        while let Some(cqe) = inner.completion().next() {
            let user_meta = cqe.user_data();
            let result = cqe.result();
            unsafe {
                let io_result = user_meta as *mut Option<i32>;
                *io_result = Some(result);
            }
        }

        Ok(())
    }
}

#[pin_project]
pub struct ReadComplection<'a, F: AsRawFd, B: AsIoVec> {
    ring: &'a Ring,

    fd: &'a F,
    buf: &'a mut B,
    offset: u64,

    #[pin]
    submited: bool,

    #[pin]
    io_result: Option<i32>, // is it thread safe?
}

impl<'a, F: AsRawFd, B: AsIoVec> ReadComplection<'a, F, B> {
    fn new(ring: &'a Ring, fd: &'a F, buf: &'a mut B, offset: u64) -> Self {
        Self {
            ring,
            fd,
            buf,
            offset,
            submited: false,
            io_result: None,
        }
    }

    fn try_push_submission_queue(self: &mut Pin<&mut Self>) -> bool {
        let mut this = self.as_mut().project();
        if *this.submited {
            return true;
        }

        let (ptr, len) = this.buf.as_io_vec();
        let offset = *this.offset;
        let fd = this.fd.as_raw_fd();

        let entry = io_uring::opcode::Read::new(types::Fd(fd), ptr, len as _)
            .offset(offset)
            .build()
            .user_data(this.io_result.get_mut() as *mut _ as _);

        if unsafe { this.ring.inner.lock().submission().push(&entry).is_err() } {
            false
        } else {
            *this.submited = true;
            true
        }
    }
}

impl<F: AsRawFd, B: AsIoVec> Future for ReadComplection<'_, F, B> {
    type Output = std::io::Result<usize>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if !self.try_push_submission_queue() {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let mut this = self.as_mut().project();
        if let Err(e) = this.ring.submit() {
            return Poll::Ready(Err(e));
        }

        // because submit is only called in one thread, so is it safety?
        if let Some(result) = this.io_result.take() {
            if result < 0 {
                Poll::Ready(Err(std::io::Error::from_raw_os_error(-result)))
            } else {
                Poll::Ready(Ok(result as usize))
            }
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}
