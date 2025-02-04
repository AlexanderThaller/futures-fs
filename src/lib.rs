#![deny(missing_docs)]
#![cfg_attr(test, deny(warnings))]
#![deny(missing_debug_implementations)]
#![doc(html_root_url = "https://docs.rs/futures-fs/0.0.5")]

//! A thread pool to handle file IO operations.
//!
//! # Examples
//!
//! ```rust
//! extern crate futures;
//! extern crate futures_fs;
//!
//! use futures::{Future, Stream};
//! use futures_fs::FsPool;
//!
//! # fn run() {
//! let fs = FsPool::default();
//!
//! // our source file
//! let read = fs.read("/home/sean/foo.txt", Default::default());
//!
//! // default writes options to create a new file
//! let write = fs.write("/home/sean/out.txt", Default::default());
//!
//! // block this thread!
//! // the reading and writing however will happen off-thread
//! read.forward(write).wait()
//!     .expect("IO error piping foo.txt to out.txt");
//! # }
//! # fn main() {}
//! ```

extern crate bytes;
#[macro_use]
extern crate futures;
extern crate futures_cpupool;

use std::path::Path;
use std::sync::Arc;
use std::{fmt, fs, io};

use futures::future::{lazy, Executor};
use futures::sync::oneshot::{self, Receiver};
use futures::{Async, Future, Poll};
use futures_cpupool::CpuPool;

pub use self::read::{FsReadStream, ReadOptions};
pub use self::write::{FsWriteSink, WriteOptions};

mod read;
mod write;

/// A pool of threads to handle file IO.
#[derive(Clone)]
pub struct FsPool {
    executor: Arc<dyn Executor<Box<dyn Future<Item = (), Error = ()> + Send>> + Send + Sync>,
}

// ===== impl FsPool ======

impl FsPool {
    /// Creates a new `FsPool`, with the supplied number of threads.
    pub fn new(threads: usize) -> Self {
        FsPool {
            executor: Arc::new(CpuPool::new(threads)),
        }
    }

    /// Creates a new `FsPool`, from an existing `Executor`.
    ///
    /// # Note
    ///
    /// The executor will be used to spawn tasks that can block the thread.
    /// It likely should not be an executor that is also handling light-weight
    /// tasks, but a dedicated thread pool.
    ///
    /// The most common use of this constructor is to allow creating a single
    /// `CpuPool` for your application for blocking tasks, and sharing it with
    /// `FsPool` and any other things needing a thread pool.
    pub fn with_executor<E>(executor: E) -> Self
    where
        E: Executor<Box<dyn Future<Item = (), Error = ()> + Send>> + Send + Sync + 'static,
    {
        FsPool {
            executor: Arc::new(executor),
        }
    }

    #[doc(hidden)]
    #[deprecated(note = "renamed to with_executor")]
    pub fn from_executor<E>(executor: E) -> Self
    where
        E: Executor<Box<dyn Future<Item = (), Error = ()> + Send>> + Send + Sync + 'static,
    {
        FsPool {
            executor: Arc::new(executor),
        }
    }

    /// Returns a `Stream` of the contents of the file at the supplied path.
    pub fn read<P>(&self, path: P, opts: ReadOptions) -> FsReadStream
    where
        P: AsRef<Path> + Send + 'static,
    {
        ::read::new(self, path, opts)
    }

    /// Returns a `Stream` of the contents of the supplied file.
    pub fn read_file(&self, file: fs::File, opts: ReadOptions) -> FsReadStream {
        ::read::new_from_file(self, file, opts)
    }

    /// Returns a `Sink` to send bytes to be written to the file at the supplied path.
    pub fn write<P>(&self, path: P, opts: WriteOptions) -> FsWriteSink
    where
        P: AsRef<Path> + Send + 'static,
    {
        ::write::new(self, path, opts)
    }

    /// Returns a `Sink` to send bytes to be written to the supplied file.
    pub fn write_file(&self, file: fs::File) -> FsWriteSink {
        ::write::new_from_file(self, file)
    }

    /// Returns a `Future` that resolves when the target file is deleted.
    pub fn delete<P>(&self, path: P) -> FsFuture<()>
    where
        P: AsRef<Path> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();

        let fut = Box::new(lazy(move || {
            tx.send(fs::remove_file(path).map_err(From::from))
                .map_err(|_| ())
        }));

        self.executor.execute(fut).unwrap();

        fs(rx)
    }
}

impl Default for FsPool {
    fn default() -> FsPool {
        FsPool::new(4)
    }
}

impl fmt::Debug for FsPool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FsPool").finish()
    }
}

// ===== impl FsFuture =====

/// A future representing work in the `FsPool`.
pub struct FsFuture<T> {
    inner: Receiver<io::Result<T>>,
}

fn fs<T: Send>(rx: Receiver<io::Result<T>>) -> FsFuture<T> {
    FsFuture { inner: rx }
}

impl<T: Send + 'static> Future for FsFuture<T> {
    type Item = T;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll().unwrap() {
            Async::Ready(Ok(item)) => Ok(Async::Ready(item)),
            Async::Ready(Err(e)) => Err(e),
            Async::NotReady => Ok(Async::NotReady),
        }
    }
}

impl<T> fmt::Debug for FsFuture<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FsFuture").finish()
    }
}

fn _assert_kinds() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    fn assert_clone<T: Clone>() {}

    assert_send::<FsPool>();
    assert_sync::<FsPool>();
    assert_clone::<FsPool>();

    assert_send::<FsFuture<()>>();
}
