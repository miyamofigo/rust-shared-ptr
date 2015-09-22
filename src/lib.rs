// This crate is an implementation of shared pointer based on
// alloc::Rc built-in Rust pointer.
// Unfortunately, though this would be the way of thinking of Rust,
// Rc doesn't allow mutable reference of memory content without
// being unique reference, so, I had to modify get_mut function 
// of Rc, in another word, get_mut was the only part to be needed to modify,
// I mean, this Shared ptr is almost Rc copy of rust lang..
// But the whole of rust lang is a little bit too huge to fork..Thank you.

#![feature(core, nonzero, alloc, unsize)]
#![feature(heap_api, filling_drop)]
#![feature(optin_builtin_traits)]
#![feature(box_syntax, box_raw)]
#![feature(coerce_unsized, core_intrinsics)]

#![crate_name = "shared"]

extern crate core;
extern crate alloc;

use std::boxed::Box;
use std::borrow;

use core::cell::Cell;
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hasher, Hash};
use core::intrinsics::{assume, drop_in_place, abort};
use core::marker::{self, Unsize};
use core::mem::{self, align_of_val, size_of_val, forget};
use core::nonzero::NonZero;
use core::ops::{CoerceUnsized, Deref};
use core::ptr;

use alloc::heap::deallocate;

struct SharedBox<T: ?Sized> {
  strong: Cell<usize>,
  weak: Cell<usize>,
  value: T,
}

pub struct Shared<T: ?Sized> {
  _ptr: NonZero<*mut SharedBox<T>>,
}

impl<T: ?Sized> !marker::Send for Shared<T> {}
impl<T: ?Sized> !marker::Sync for Shared<T> {}

impl<T: ?Sized+Unsize<U>, U: ?Sized> CoerceUnsized<Shared<U>> for Shared<T> {}

impl<T> Shared<T> {

  pub fn new(value: T) -> Shared<T> {
    unsafe {
      Shared { 
        _ptr: NonZero::new(Box::into_raw(box SharedBox {
          strong: Cell::new(1),
          weak: Cell::new(1),
          value: value
        })), 
      }
    }
  }

  #[inline]
  pub fn try_unwrap(this: Self) -> Result<T, Self> {
    if Shared::would_unwrap(&this) {
      unsafe {
        let val = ptr::read(&*this);
        this.dec_strong();
        let _weak = Weak2 { _ptr: this._ptr };
        forget(this);
        Ok(val)
      }
    } else {
      Err(this)
    }
  } 

  pub fn would_unwrap(this: &Self) -> bool {
    Shared::strong_count(&this) == 1
  } 

}

impl<T: ?Sized> Shared<T> {

  pub fn downgrade(this: &Self) -> Weak2<T> {
    this.inc_weak();
    Weak2 { _ptr: this._ptr }
  }  

  #[inline]
  pub fn weak_count(this: &Self) -> usize { this.weak() - 1 } 

  #[inline]
  pub fn strong_count(this: &Self) -> usize { this.strong() }

  #[inline]
  pub fn is_unique(this: &Self) -> bool {
    Shared::weak_count(this) == 0 && Shared::strong_count(this) == 1 
  } 

  #[inline]
  pub fn get_mut(this: &mut Self) -> Option<&mut T> {
    // If not being unique, get_mut function works!
    let inner = unsafe { &mut **this._ptr };
    Some(&mut inner.value)
  }

}

impl <T: Clone> Shared<T> {

  #[inline]
  pub fn make_mut(this: &mut Self) -> &mut T {

    if Shared::strong_count(this) != 1 {
      *this = Shared::new((**this).clone())
    } else if Shared::weak_count(this) != 0 {
      unsafe {
        let mut swap = Shared::new(ptr::read(&(**this._ptr).value));
        mem::swap(this, &mut swap);
        swap.dec_strong();
        swap.dec_weak();
        forget(swap);
      }
    }

    let inner = unsafe { &mut **this._ptr };
    &mut inner.value
  }

}

impl<T: ?Sized> Deref for Shared<T> {

  type Target = T;

  #[inline]
  fn deref(&self) -> &T {
    &self.inner().value
  }
  
}

impl<T: ?Sized> Drop for Shared<T> {

  fn drop(&mut self) {

    unsafe {
      let ptr = *self._ptr;
      if !(*(&ptr as *const _ as *const *const ())).is_null() &&
          ptr as *const () as usize != mem::POST_DROP_USIZE {
        self.dec_strong();
        if self.strong() == 0 {
          drop_in_place(&mut (*ptr).value);
          self.dec_weak();
          if self.weak() == 0 {
            deallocate(ptr as *mut u8,
                       size_of_val(&*ptr),
                       align_of_val(&*ptr))
          }
        }
      }
    }
  }

}

impl<T: ?Sized> Clone for Shared<T> {

  #[inline]
  fn clone(&self) -> Shared<T> {
    self.inc_strong();
    Shared { _ptr: self._ptr }
  }
}

impl<T: Default> Default for Shared<T> {
  
  #[inline]
  fn default() -> Shared<T> {
    Shared::new(Default::default())
  }

}

impl<T: ?Sized + PartialEq> PartialEq for Shared<T> {

  #[inline]
  fn eq(&self, other: &Shared<T>) -> bool { **self == **other }
  
  #[inline]
  fn ne(&self, other: &Shared<T>) -> bool { **self != **other }

}

impl<T: ?Sized + Eq> Eq for Shared<T> {}

impl<T: ?Sized + PartialOrd> PartialOrd for Shared<T> {

  #[inline(always)]
  fn partial_cmp(&self, other: &Shared<T>) -> Option<Ordering> {
    (**self).partial_cmp(&**other)
  }

  #[inline(always)]
  fn lt(&self, other: &Shared<T>) -> bool { **self < **other }

  #[inline(always)]
  fn le(&self, other: &Shared<T>) -> bool { **self <= **other }

  #[inline(always)]
  fn gt(&self, other: &Shared<T>) -> bool { **self > **other }

  #[inline(always)]
  fn ge(&self, other: &Shared<T>) -> bool { **self >= **other }

}

impl<T: ?Sized + Ord> Ord for Shared<T> {
  
  #[inline]
  fn cmp(&self, other: &Shared<T>) -> Ordering { (**self).cmp(&**other) }

}

impl<T: ?Sized + Hash> Hash for Shared<T> {

  fn hash<H: Hasher>(&self, state: &mut H) {
    (**self).hash(state);
  }

}

impl<T: ?Sized + fmt::Display> fmt::Display for Shared<T> {

  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::Display::fmt(&**self, f)
  }

}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Shared<T> {

  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::Debug::fmt(&**self, f)
  }

}

impl<T> fmt::Pointer for Shared<T> {

  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::Pointer::fmt(&*self._ptr, f)
  }

} 

impl<T: ?Sized> borrow::Borrow<T> for Shared<T> {

  fn borrow(&self) -> &T { &**self }
}

trait SharedBoxPtr<T: ?Sized> {

  fn inner(&self) -> &SharedBox<T>;

  #[inline]
  fn strong(&self) -> usize { self.inner().strong.get() }

  #[inline]
  fn inc_strong(&self) {
    self.inner().strong.set(
      self.strong()
          .checked_add(1)
          .unwrap_or_else( || unsafe { abort() } ));
  }

  #[inline]
  fn dec_strong(&self) { self.inner().strong.set(self.strong() - 1); }

  #[inline]
  fn weak(&self) -> usize { self.inner().weak.get() }

  #[inline]
  fn inc_weak(&self) {
    self.inner().weak.set(
      self.weak()
          .checked_add(1)
          .unwrap_or_else( || unsafe { abort() } ));
  }
  
  #[inline]
  fn dec_weak(&self) { self.inner().weak.set(self.weak() - 1); }

}
    
impl<T: ?Sized> SharedBoxPtr<T> for Shared<T> {

  #[inline(always)]
  fn inner(&self) -> &SharedBox<T> {
    unsafe {
      assume(!(*(&self._ptr as *const _ as *const *const ())).is_null());
      &(**self._ptr)
    }
  }

}

pub struct Weak2<T: ?Sized> {
  _ptr: NonZero<*mut SharedBox<T>>,
}

impl<T: ?Sized> !marker::Send for Weak2<T> {}
impl<T: ?Sized> !marker::Sync for Weak2<T> {}

impl<T: ?Sized+Unsize<U>, U: ?Sized> CoerceUnsized<Weak2<U>> for Weak2<T> {}

impl<T: ?Sized> Weak2<T> {

  pub fn upgrade(&self) -> Option<Shared<T>> {

    if self.strong() == 0 { None }
    else { 
      self.inc_strong();
      Some(Shared { _ptr: self._ptr })
    }
  }

}

impl<T: ?Sized> SharedBoxPtr<T> for Weak2<T> {

  #[inline(always)]
  fn inner(&self) -> &SharedBox<T> {
    unsafe {
      assume(!(*(&self._ptr as *const _ as *const *const ())).is_null());
      &(**self._ptr)
    }
  }

}

impl<T: ?Sized> Drop for Weak2<T> { 

  fn drop(&mut self) {

    unsafe {
      let ptr = *self._ptr;
      if !(*(&ptr as *const _ as *const *const ())).is_null() &&
        ptr as *const () as usize != mem::POST_DROP_USIZE {
        self.dec_weak();
        if self.weak() == 0 {
          deallocate(ptr as *mut u8,
                     size_of_val(&*ptr),
                     align_of_val(&*ptr))
        }
      }
    }
  }
}

impl<T:?Sized> Clone for Weak2<T> {
  
  #[inline]
  fn clone(&self) -> Weak2<T> {
    self.inc_weak();
    Weak2 { _ptr: self._ptr }
  }
}

impl <T: ?Sized+fmt::Debug> fmt::Debug for Weak2<T> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "(Weak2)")
  }
}

#[cfg(test)]
mod tests {
  
  use super::{Shared, Weak2};
  use std::boxed::Box;
  use std::cell::RefCell;
  use std::option::Option;
  use std::option::Option::{Some, None};
  use std::result::Result::{Err, Ok};
  use std::mem::drop;
  use std::clone::Clone;
  
  #[test]
  fn test_clone() {
    let x = Shared::new(RefCell::new(5));
    let y = x.clone();
    *x.borrow_mut() = 20;
    assert_eq!(*y.borrow(), 20);
  }

  #[test]
  fn test_simple() {
    let x = Shared::new(5);
    assert_eq!(*x, 5);
  }

  #[test]
  fn test_simple_clone() {
    let x = Shared::new(5);
    let y = x.clone();
    assert_eq!(*x, 5);
    assert_eq!(*y, 5);
  }

  #[test]
  fn test_destructor() {
    let x: Shared<Box<_>> = Shared::new(box 5);
    assert_eq!(**x, 5);
  }

  #[test]
  fn test_live() {
    let x = Shared::new(5);
    let y = Shared::downgrade(&x);
    assert!(y.upgrade().is_some());
  }
  
  #[test]
  fn test_dead() {
    let x = Shared::new(5);
    let y = Shared::downgrade(&x);
    drop(x);
    assert!(y.upgrade().is_none());
  }

  #[test]
  fn weak_self_cyclic() {

    struct Cycle {
      x: RefCell<Option<Weak2<Cycle>>>
    }

    let a = Shared::new(Cycle { x: RefCell::new(None) });
    let b = Shared::downgrade(&a.clone());
    *a.x.borrow_mut() = Some(b);
  }
  
  #[test]
  fn test_weak_count() { 

    let a = Shared::new(0u32);
    assert!(Shared::strong_count(&a) == 1);
    assert!(Shared::weak_count(&a) == 0);
    let w = Shared::downgrade(&a);
    assert!(Shared::strong_count(&a) == 1); 
    assert!(Shared::weak_count(&a) == 1); 
    drop(w);
    assert!(Shared::strong_count(&a) == 1);
    assert!(Shared::weak_count(&a) == 0); 
    let c = a.clone();
    assert!(Shared::strong_count(&a) == 2);
    assert!(Shared::weak_count(&a) == 0); 
    drop(c);  

  } 

  #[test]
  fn try_unwrap() {

    let x = Shared::new(3);
    assert_eq!(Shared::try_unwrap(x), Ok(3));
    let x = Shared::new(4);
    let _y = x.clone();
    assert_eq!(Shared::try_unwrap(x), Err(Shared::new(4)));
    let x = Shared::new(5);
    let _w = Shared::downgrade(&x);
    assert_eq!(Shared::try_unwrap(x), Ok(5));

  }

  #[test]
  fn get_mut() {
    let mut x = Shared::new(3);
    *Shared::get_mut(&mut x).unwrap() = 4;
    assert_eq!(*x, 4);
    let y = x.clone();
    assert!(Shared::get_mut(&mut x).is_some());
  }

  #[test]
  fn test_cowshared_clone_make_unique() {

    let mut cow0 = Shared::new(75);
    let mut cow1 = cow0.clone();
    let mut cow2 = cow1.clone();
    
    assert!(75 == *Shared::make_mut(&mut cow0));
    assert!(75 == *Shared::make_mut(&mut cow1));
    assert!(75 == *Shared::make_mut(&mut cow2));
  
    *Shared::make_mut(&mut cow0) += 1;
    *Shared::make_mut(&mut cow1) += 2;
    *Shared::make_mut(&mut cow2) += 3;

    assert!(76 == *cow0);
    assert!(77 == *cow1);
    assert!(78 == *cow2);
  
    assert!(*cow0 != *cow1);  
    assert!(*cow0 != *cow2);  
    assert!(*cow1 != *cow2);  

  }

  #[test]
  fn test_cowshared_clone_make_unique2() {

    let mut cow0 = Shared::new(75);
    let cow1 = cow0.clone();
    let cow2 = cow1.clone();

    assert!(75 == *cow0);
    assert!(75 == *cow1);
    assert!(75 == *cow2);
  
    *Shared::make_mut(&mut cow0) += 1;
  
    assert!(76 == *cow0); 
    assert!(75 == *cow1); 
    assert!(75 == *cow2); 
  
    assert!(*cow0 != *cow1);  
    assert!(*cow0 != *cow2);  
    assert!(*cow1 == *cow2);  

  }
  
  #[test]
  fn test_cowshared_clone_weak() {

    let mut cow0 = Shared::new(75);
    let cow1_weak = Shared::downgrade(&cow0);

    assert!(75 == *cow0);
    assert!(75 == *cow1_weak.upgrade().unwrap());

    *Shared::make_mut(&mut cow0) += 1;
  
    assert!(76 == *cow0);
    assert!(cow1_weak.upgrade().is_none());
    
  }

  #[test]
  fn test_show() {
    let foo = Shared::new(75);
    assert_eq!(format!("{:?}", foo), "75");
  }

  #[test]
  fn test_unsized() {
    let foo: Shared<[i32]> = Shared::new([1, 2, 3]);
    assert_eq!(foo, foo.clone());
  }

}
