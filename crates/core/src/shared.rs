//! Wire-transparent copy-on-write storage for document branches. Snapshot clones
//! retain bodies and inline arrays; only a branch requested mutably detaches.
//! This is NOT a persistent indexed tree: detaching a block vector still copies
//! its block headers, but not every unchanged paragraph's inline text.
use std::{ops::{Deref,DerefMut},sync::Arc};
use serde::{Serialize,Serializer,Deserialize,Deserializer};

#[derive(Debug)]
pub struct Shared<T>(Arc<T>);
impl<T> Shared<T> {
    pub fn shares_storage(&self, other:&Self)->bool { Arc::ptr_eq(&self.0,&other.0) }
}
impl<T:Clone> Shared<T> {
    pub fn into_owned(self)->T { Arc::try_unwrap(self.0).unwrap_or_else(|shared|(*shared).clone()) }
}
impl<T> Clone for Shared<T> { fn clone(&self)->Self { Self(Arc::clone(&self.0)) } }
impl<T:Default> Default for Shared<T> { fn default()->Self { Self(Arc::new(T::default())) } }
impl<T> From<T> for Shared<T> { fn from(value:T)->Self { Self(Arc::new(value)) } }
impl<T> Deref for Shared<T> { type Target=T; fn deref(&self)->&T { &self.0 } }
impl<T:Clone> DerefMut for Shared<T> { fn deref_mut(&mut self)->&mut T { Arc::make_mut(&mut self.0) } }
impl<T:PartialEq> PartialEq for Shared<T> { fn eq(&self,other:&Self)->bool { self.shares_storage(other)||**self==**other } }
impl<T:Eq> Eq for Shared<T> {}
impl<T:Serialize> Serialize for Shared<T> {
    fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error> { (**self).serialize(s) }
}
impl<'de,T:Deserialize<'de>> Deserialize<'de> for Shared<T> {
    fn deserialize<D:Deserializer<'de>>(d:D)->Result<Self,D::Error> { T::deserialize(d).map(Self::from) }
}
impl<T:Clone> Shared<Vec<T>> {
    /// Clearing a shared branch must not first duplicate its discarded content.
    pub fn clear(&mut self) {
        if let Some(vec)=Arc::get_mut(&mut self.0){vec.clear();}else{self.0=Arc::new(Vec::new());}
    }
}
impl<T:Clone> From<Shared<Vec<T>>> for Vec<T> { fn from(value:Shared<Vec<T>>)->Self { value.into_owned() } }
impl<T> FromIterator<T> for Shared<Vec<T>> { fn from_iter<I:IntoIterator<Item=T>>(iter:I)->Self { Vec::from_iter(iter).into() } }
impl<T:Clone> IntoIterator for Shared<Vec<T>> {
    type Item=T;type IntoIter=std::vec::IntoIter<T>;
    fn into_iter(self)->Self::IntoIter { self.into_owned().into_iter() }
}
impl<'a,T> IntoIterator for &'a Shared<Vec<T>> {
    type Item=&'a T;type IntoIter=std::slice::Iter<'a,T>;
    fn into_iter(self)->Self::IntoIter { self.iter() }
}
impl<'a,T:Clone> IntoIterator for &'a mut Shared<Vec<T>> {
    type Item=&'a mut T;type IntoIter=std::slice::IterMut<'a,T>;
    fn into_iter(self)->Self::IntoIter { self.iter_mut() }
}

#[cfg(test)] #[path = "../tests/unit/shared.rs"] mod tests;

