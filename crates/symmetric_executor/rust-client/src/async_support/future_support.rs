use std::{
    future::{Future, IntoFuture},
    marker::PhantomData,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll},
};

use futures::FutureExt;

use crate::symmetric_stream::{Address, Buffer};

use super::{wait_on, Stream};

pub struct FutureWriter<T: 'static> {
    handle: Stream,
    // future: Option<Pin<Box<dyn Future<Output = ()> + 'static + Send>>>,
    _phantom: PhantomData<T>,
    lower: unsafe fn(value: T, dst: *mut u8),
}

impl<T> FutureWriter<T> {
    pub fn new(handle: Stream, lower: unsafe fn(value: T, dst: *mut u8)) -> Self {
        Self {
            handle,
            // future: None,
            _phantom: PhantomData,
            lower,
        }
    }

    pub fn write(self, data: T) -> CancelableWrite<T> {
        CancelableWrite {
            writer: self,
            future: None,
            data: Some(data),
        }
    }
}

/// Represents a write operation which may be canceled prior to completion.
pub struct CancelableWrite<T: 'static> {
    writer: FutureWriter<T>,
    future: Option<Pin<Box<dyn Future<Output = ()> + 'static + Send>>>,
    data: Option<T>,
}

impl<T: Unpin + Send> Future for CancelableWrite<T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let me = self.get_mut();

        if me.future.is_none() {
            let handle = me.writer.handle.clone();
            let data = me.data.take().unwrap();
            let lower = me.writer.lower;
            me.future = Some(Box::pin(async move {
                if !handle.is_ready_to_write() {
                    let subsc = handle.write_ready_subscribe();
                    wait_on(subsc).await;
                }
                let buffer = handle.start_writing();
                let addr = buffer.get_address().take_handle() as *mut MaybeUninit<T> as *mut u8;
                unsafe { (lower)(data, addr) };
                buffer.set_size(1);
                handle.finish_writing(Some(buffer));
            }) as Pin<Box<dyn Future<Output = _> + Send>>);
        }
        me.future.as_mut().unwrap().poll_unpin(cx)
    }
}

/// Represents a read operation which may be canceled prior to completion.
pub struct CancelableRead<T: 'static> {
    reader: FutureReader<T>,
    future: Option<Pin<Box<dyn Future<Output = Option<T>> + 'static + Send>>>,
}

pub struct FutureReader<T: 'static> {
    handle: Stream,
    // future: Option<Pin<Box<dyn Future<Output = Option<Vec<T>>> + 'static + Send>>>,
    _phantom: PhantomData<T>,
    lift: unsafe fn(src: *const u8) -> T,
}

impl<T> FutureReader<T> {
    pub fn new(handle: Stream, lift: unsafe fn(src: *const u8) -> T) -> Self {
        Self {
            handle,
            // future: None,
            _phantom: PhantomData,
            lift,
        }
    }

    pub fn read(self) -> CancelableRead<T> {
        CancelableRead {
            reader: self,
            future: None,
        }
    }

    pub unsafe fn from_handle(handle: *mut u8, lift: unsafe fn(src: *const u8) -> T) -> Self {
        Self::new(unsafe { Stream::from_handle(handle as usize) }, lift)
    }

    pub fn take_handle(&self) -> *mut () {
        self.handle.take_handle() as *mut ()
    }
}

impl<T: Unpin + Sized + Send> Future for CancelableRead<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<T>> {
        let me = self.get_mut();

        if me.future.is_none() {
            let handle = me.reader.handle.clone();
            let lift = me.reader.lift;
            me.future = Some(Box::pin(async move {
                let mut buffer0 = MaybeUninit::<T>::uninit();
                let address = unsafe { Address::from_handle(&mut buffer0 as *mut _ as usize) };
                let buffer = Buffer::new(address, 1);
                handle.start_reading(buffer);
                let subsc = handle.read_ready_subscribe();
                subsc.reset();
                wait_on(subsc).await;
                let buffer2 = handle.read_result();
                if let Some(buffer2) = buffer2 {
                    let count = buffer2.get_size();
                    if count > 0 {
                        Some(unsafe { (lift)(buffer2.get_address().take_handle() as *const u8) })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }) as Pin<Box<dyn Future<Output = _> + Send>>);
        }

        match me.future.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Ready(v) => {
                me.future = None;
                Poll::Ready(v)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> CancelableRead<T> {
    pub fn cancel(mut self) -> FutureReader<T> {
        self.cancel_mut()
    }

    fn cancel_mut(&mut self) -> FutureReader<T> {
        todo!()
    }
}

impl<T: Send + Unpin + Sized> IntoFuture for FutureReader<T> {
    type Output = Option<T>;
    type IntoFuture = CancelableRead<T>;

    /// Convert this object into a `Future` which will resolve when a value is
    /// written to the writable end of this `future` (yielding a `Some` result)
    /// or when the writable end is dropped (yielding a `None` result).
    fn into_future(self) -> Self::IntoFuture {
        self.read()
    }
}

pub fn new_future<T: 'static>(
    lower: unsafe fn(value: T, dst: *mut u8),
    lift: unsafe fn(src: *const u8) -> T,
) -> (FutureWriter<T>, FutureReader<T>) {
    let handle = Stream::new();
    let handle2 = handle.clone();
    (
        FutureWriter::new(handle, lower),
        FutureReader::new(handle2, lift),
    )
}
