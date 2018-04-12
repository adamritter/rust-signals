use super::signal::Signal;
// TODO use parking_lot ?
use std::sync::{Arc, RwLock, Mutex, MutexGuard, RwLockReadGuard};
use futures_core::Async;
use futures_core::task::Context;


#[inline]
pub fn lock_mut<A>(x: &Mutex<A>) -> MutexGuard<A> {
    x.lock().unwrap()
}

#[inline]
pub fn lock_ref<A>(x: &RwLock<A>) -> RwLockReadGuard<A> {
    x.read().unwrap()
}

pub fn unwrap_mut<A>(x: &mut Option<A>) -> &mut A {
    match *x {
        Some(ref mut x) => x,
        None => unreachable!(),
    }
}

pub fn unwrap_ref<A>(x: &Option<A>) -> &A {
    match *x {
        Some(ref x) => x,
        None => unreachable!(),
    }
}


pub struct Map2<A: Signal, B: Signal, C> {
    signal1: Option<A>,
    signal2: Option<B>,
    callback: C,
    left: Option<A::Item>,
    right: Option<B::Item>,
}

impl<A, B, C, D> Map2<A, B, C>
    where A: Signal,
          B: Signal,
          C: FnMut(&mut A::Item, &mut B::Item) -> D {
    #[inline]
    pub fn new(left: A, right: B, callback: C) -> Self {
        Self {
            signal1: Some(left),
            signal2: Some(right),
            callback,
            left: None,
            right: None,
        }
    }
}

// Poll left  => Has left   => Poll right  => Has right   => Output
// -----------------------------------------------------------------------------
// Some(left) =>            => Some(right) =>             => Some((left, right))
// Some(left) =>            => None        => Some(right) => Some((left, right))
// Some(left) =>            => Pending     => Some(right) => Some((left, right))
// None       => Some(left) => Some(right) =>             => Some((left, right))
// Pending    => Some(left) => Some(right) =>             => Some((left, right))
// None       => Some(left) => None        => Some(right) => None
// None       => None       =>             =>             => None
//            =>            => None        => None        => None
// Some(left) =>            => Pending     => None        => Pending
// None       => Some(left) => Pending     => Some(right) => Pending
// None       => Some(left) => Pending     => None        => Pending
// Pending    => Some(left) => None        => Some(right) => Pending
// Pending    => Some(left) => Pending     => Some(right) => Pending
// Pending    => Some(left) => Pending     => None        => Pending
// Pending    => None       => Some(right) =>             => Pending
// Pending    => None       => None        => Some(right) => Pending
// Pending    => None       => Pending     => Some(right) => Pending
// Pending    => None       => Pending     => None        => Pending
impl<A, B, C, D> Signal for Map2<A, B, C>
    where A: Signal,
          B: Signal,
          C: FnMut(&mut A::Item, &mut B::Item) -> D {
    type Item = D;

    // TODO inline this ?
    fn poll_change(&mut self, cx: &mut Context) -> Async<Option<Self::Item>> {
        let mut changed = false;

        let left_done = match self.signal1.as_mut().map(|signal| signal.poll_change(cx)) {
            None => true,
            Some(Async::Ready(None)) => {
                self.signal1 = None;
                true
            },
            Some(Async::Ready(a)) => {
                self.left = a;
                changed = true;
                false
            },
            Some(Async::Pending) => false,
        };

        let left = match self.left {
            Some(ref mut left) => left,

            None => if left_done {
                return Async::Ready(None);

            } else {
                return Async::Pending;
            },
        };

        let right_done = match self.signal2.as_mut().map(|signal| signal.poll_change(cx)) {
            None => true,
            Some(Async::Ready(None)) => {
                self.signal2 = None;
                true
            },
            Some(Async::Ready(a)) => {
                self.right = a;
                changed = true;
                false
            },
            Some(Async::Pending) => false,
        };

        let right = match self.right {
            Some(ref mut right) => right,

            None => if right_done {
                return Async::Ready(None);

            } else {
                return Async::Pending;
            },
        };

        if changed {
            Async::Ready(Some((self.callback)(left, right)))

        } else if left_done && right_done {
            Async::Ready(None)

        } else {
            Async::Pending
        }
    }
}


// TODO is it possible to avoid the Arc ?
// TODO is it possible to use only a single Mutex ?
pub type PairMut<A, B> = Arc<(Mutex<Option<A>>, Mutex<Option<B>>)>;

pub struct MapPairMut<A: Signal, B: Signal> {
    signal1: Option<A>,
    signal2: Option<B>,
    inner: PairMut<A::Item, B::Item>,
}

impl<A, B> MapPairMut<A, B>
    where A: Signal,
          B: Signal {
    #[inline]
    pub fn new(left: A, right: B) -> Self {
        Self {
            signal1: Some(left),
            signal2: Some(right),
            inner: Arc::new((Mutex::new(None), Mutex::new(None))),
        }
    }
}

impl<A, B> Signal for MapPairMut<A, B>
    where A: Signal,
          B: Signal {
    type Item = PairMut<A::Item, B::Item>;

    // TODO inline this ?
    fn poll_change(&mut self, cx: &mut Context) -> Async<Option<Self::Item>> {
        let mut changed = false;

        // TODO can this deadlock ?
        let mut borrow_left = self.inner.0.lock().unwrap();

        // TODO is it okay to move this to just above right_done ?
        let mut borrow_right = self.inner.1.lock().unwrap();

        let left_done = match self.signal1.as_mut().map(|signal| signal.poll_change(cx)) {
            None => true,
            Some(Async::Ready(None)) => {
                self.signal1 = None;
                true
            },
            Some(Async::Ready(a)) => {
                *borrow_left = a;
                changed = true;
                false
            },
            Some(Async::Pending) => false,
        };

        if borrow_left.is_none() {
            if left_done {
                return Async::Ready(None);

            } else {
                return Async::Pending;
            }
        }

        let right_done = match self.signal2.as_mut().map(|signal| signal.poll_change(cx)) {
            None => true,
            Some(Async::Ready(None)) => {
                self.signal2 = None;
                true
            },
            Some(Async::Ready(a)) => {
                *borrow_right = a;
                changed = true;
                false
            },
            Some(Async::Pending) => false,
        };

        if borrow_right.is_none() {
            if right_done {
                return Async::Ready(None);

            } else {
                return Async::Pending;
            }
        }

        if changed {
            Async::Ready(Some(self.inner.clone()))

        } else if left_done && right_done {
            Async::Ready(None)

        } else {
            Async::Pending
        }
    }
}


// TODO is it possible to avoid the Arc ?
// TODO maybe it's faster to use a Mutex ?
pub type Pair<A, B> = Arc<RwLock<(Option<A>, Option<B>)>>;

pub struct MapPair<A: Signal, B: Signal> {
    signal1: Option<A>,
    signal2: Option<B>,
    inner: Pair<A::Item, B::Item>,
}

impl<A, B> MapPair<A, B>
    where A: Signal,
          B: Signal {
    #[inline]
    pub fn new(left: A, right: B) -> Self {
        Self {
            signal1: Some(left),
            signal2: Some(right),
            inner: Arc::new(RwLock::new((None, None))),
        }
    }
}

impl<A, B> Signal for MapPair<A, B>
    where A: Signal,
          B: Signal {
    type Item = Pair<A::Item, B::Item>;

    // TODO inline this ?
    fn poll_change(&mut self, cx: &mut Context) -> Async<Option<Self::Item>> {
        let mut changed = false;

        let mut borrow = self.inner.write().unwrap();

        let left_done = match self.signal1.as_mut().map(|signal| signal.poll_change(cx)) {
            None => true,
            Some(Async::Ready(None)) => {
                self.signal1 = None;
                true
            },
            Some(Async::Ready(a)) => {
                borrow.0 = a;
                changed = true;
                false
            },
            Some(Async::Pending) => false,
        };

        if borrow.0.is_none() {
            if left_done {
                return Async::Ready(None);

            } else {
                return Async::Pending;
            }
        }

        let right_done = match self.signal2.as_mut().map(|signal| signal.poll_change(cx)) {
            None => true,
            Some(Async::Ready(None)) => {
                self.signal2 = None;
                true
            },
            Some(Async::Ready(a)) => {
                borrow.1 = a;
                changed = true;
                false
            },
            Some(Async::Pending) => false,
        };

        if borrow.1.is_none() {
            if right_done {
                return Async::Ready(None);

            } else {
                return Async::Pending;
            }
        }

        if changed {
            Async::Ready(Some(self.inner.clone()))

        } else if left_done && right_done {
            Async::Ready(None)

        } else {
            Async::Pending
        }
    }
}
