// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime-agnostic future and stream types, std only.
//!
//! The harness never names an async runtime. Traits return these boxed
//! types; whoever implements them picks the runtime.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A boxed, sendable stream.
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

/// A sequence of values produced asynchronously.
pub trait Stream {
    /// What the stream yields.
    type Item;

    /// Poll for the next item; `Ready(None)` ends the stream.
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
}

/// A future that is already complete.
pub fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(std::future::ready(value))
}

/// The next item of a stream, as a future.
pub fn next<S: Stream + Unpin + ?Sized>(stream: &mut S) -> Next<'_, S> {
    Next { stream }
}

/// The future returned by [`next`].
pub struct Next<'a, S: ?Sized> {
    stream: &'a mut S,
}

impl<S: Stream + Unpin + ?Sized> Future for Next<'_, S> {
    type Output = Option<S::Item>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.stream).poll_next(cx)
    }
}

impl<S: Stream + ?Sized> Stream for Pin<Box<S>> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().as_mut().poll_next(cx)
    }
}

/// A stream over items already in hand.
pub fn iter<T: Send + 'static>(items: Vec<T>) -> BoxStream<'static, T> {
    Box::pin(Iter(items.into_iter()))
}

struct Iter<T>(std::vec::IntoIter<T>);

impl<T> Stream for Iter<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<T>> {
        Poll::Ready(self.get_mut().0.next())
    }
}

impl<T> Unpin for Iter<T> {}

/// FNV-1a, 64-bit, as 16 lowercase hex digits. Used for fixture and request
/// digests; not for security.
pub fn fnv1a64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
