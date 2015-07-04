use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::hash::Hash;
use core::cmp::Eq;
use super::SessionStore;

type Store<K, V> = RwLock<HashMap<K, RwLock<V>>>;

/// A default implementation of `SessionStore`.
///
/// Session store implemented as a read-write-locked `HashMap`.
///
/// #### To use:
/// ```ignore
/// // When defining your server:
/// server.link(Sessions::new(key_gen_fn, HashSessionStore::<KeyType, ValueType>::new()));
///
/// // When accessing from your middleware:
/// let session = alloy.find_mut::<Session<KeyType, ValueType>>().unwrap();
/// ```
pub struct HashSessionStore<K, V>{
    store: Arc<Store<K, V>>
}

impl<K: Clone + Send, V: Send> Clone for HashSessionStore<K, V> {
    fn clone(&self) -> HashSessionStore<K, V> {
        HashSessionStore {
            store: self.store.clone()
        }
    }
}

impl<K: Hash + Eq + Send + Sync, V: Send + Sync> HashSessionStore<K, V> {
    /// Create a new instance of the session store
    pub fn new() -> HashSessionStore<K, V> {
        HashSessionStore {
            store: Arc::new(RwLock::new(HashMap::<K, RwLock<V>>::new()))
        }
    }
}

/* A note on clones:
 *
 * Those values hidden behind a RwLock are owned behind that lock.
 * In order for them to be accessed, a reference to the two gating locks
 * (the HashMap and the keyed V) must be kept alive.
 *
 * Instead, all values returned are copies.
 */
impl<K: Hash + Eq + Send + Sync + Clone, V: Send + Sync + Clone> SessionStore<K, V> for HashSessionStore<K, V> {
    fn insert(&self, key: &K, val: V) {
        // Avoid a WriteLock if possible
        if !self.store.read().contains_key(key) {
            // Inserting consumes a key => clone()
            self.store.write().insert(key.clone(), RwLock::new(val));
        }
    }
    fn find(&self, key: &K) -> Option<V> {
        match self.store.read().find(key) {
            Some(lock) => Some(lock.read().clone()),
            None => None
        }
    }
    fn swap(&self, key: &K, value: V) -> Option<V> {
        match self.store.read().find(key) {
            // Instead of using swap, which requires a write lock on the HashMap,
            // only take the write locks when the key does not yet exist
            Some(lock) => {
                let old_v = lock.read().clone();
                *lock.write() = value;
                return Some(old_v)
            },
            None => ()
        }
        self.insert(key, value);
        None
    }
    fn upsert<F>(&self, key: &K, value: V, mutator: F) -> V
      where F: Fn(&mut V) -> () {
        match self.store.read().find(key) {
            Some(lock) => {
                let old_v = &mut *lock.write();
                mutator(old_v);
                return old_v.clone()
            },
            None => ()
        }
        self.insert(key, value.clone());
        value
    }
    fn remove(&self, key: &K) -> bool {
        self.store.write().remove(key)
    }
}

#[cfg(test)]
mod test {
    pub use super::*;
    pub use super::super::*;
    pub use super::super::session::*;
    pub use super::super::super::sessions::*;
    pub use iron::*;
    pub use test::mock::{request, response};

    pub fn set_server() -> Server {
        let mut test_server: Server = Iron::new();
        test_server.chain.link(Sessions::new(get_session_id, HashSessionStore::<char, char>::new()));
        test_server
    }
    pub fn run_server(mut server: Server) {
        let _ = server.chain.dispatch(
            &mut request::new(::http::method::Get, "localhost:3000"),
            &mut response::new());
    }

    pub fn get_session_id(_: &Request) -> char {'a'}

    pub fn set_session_to_a(req: &mut Request, _: &mut Response) -> Status {
        let session = req.alloy.find::<Session<char, char>>().unwrap();
        session.insert('a');
        Continue
    }
    pub fn set_session_to_b(req: &mut Request, _: &mut Response) -> Status {
        let session = req.alloy.find::<Session<char, char>>().unwrap();
        session.insert('b');
        Continue
    }
    pub fn swap_session_to_b(req: &mut Request, _: &mut Response) -> Status {
        let session = req.alloy.find::<Session<char, char>>().unwrap();
        session.swap('b');
        Continue
    }
    pub fn upsert_session(req: &mut Request, _: &mut Response) -> Status {
        let session = req.alloy.find::<Session<char, char>>().unwrap();
        let _ = session.upsert('b', |c: &mut char| *c = 'a');
        Continue
    }
    pub fn remove_session(req: &mut Request, _: &mut Response) -> Status {
        let session = req.alloy.find::<Session<char, char>>().unwrap();
        session.remove();
        Continue
    }
    pub fn check_session_is_not_set(req: &mut Request, _: &mut Response) -> Status {
        let session = req.alloy.find::<Session<char, char>>().unwrap();
        assert_eq!(session.find(), None);
        Continue
    }
    pub fn check_session_is_set_to_a(req: &mut Request, _: &mut Response) -> Status {
        let session = req.alloy.find::<Session<char, char>>().unwrap();
        assert_eq!(session.find(), Some('a'));
        Continue
    }
    pub fn check_session_is_set_to_b(req: &mut Request, _: &mut Response) -> Status {
        let session = req.alloy.find::<Session<char, char>>().unwrap();
        assert_eq!(session.find(), Some('b'));
        Continue
    }

    mod enter {
        pub use super::*;

        #[test]
        fn starts_with_empty_session() {
            let mut test_server = set_server();
            test_server.chain.link(FromFn::new(check_session_is_not_set));
            run_server(test_server);
        }

        #[test]
        fn finds_session() {
            let mut test_server = set_server();
            test_server.chain.link(FromFn::new(set_session_to_a));
            test_server.chain.link(FromFn::new(check_session_is_set_to_a));
            run_server(test_server);
        }

        mod swap {
            use super::*;

            #[test]
            fn swaps_session_when_empty() {
                let mut test_server = set_server();
                test_server.chain.link(FromFn::new(swap_session_to_b));
                test_server.chain.link(FromFn::new(check_session_is_set_to_b));
                run_server(test_server);
            }

            #[test]
            fn swaps_session_when_non_empty() {
                let mut test_server = set_server();
                test_server.chain.link(FromFn::new(set_session_to_a));
                test_server.chain.link(FromFn::new(swap_session_to_b));
                test_server.chain.link(FromFn::new(check_session_is_set_to_b));
                run_server(test_server);
            }


            #[test]
            fn swaps_session_when_same_valued() {
                let mut test_server = set_server();
                test_server.chain.link(FromFn::new(set_session_to_b));
                test_server.chain.link(FromFn::new(swap_session_to_b));
                test_server.chain.link(FromFn::new(check_session_is_set_to_b));
                run_server(test_server);
            }
        }

        mod upsert {
            use super::*;

            #[test]
            fn inserts_session_when_empty() {
                let mut test_server = set_server();
                test_server.chain.link(FromFn::new(upsert_session));
                test_server.chain.link(FromFn::new(check_session_is_set_to_b));
                run_server(test_server);
            }

            #[test]
            fn mutates_session_when_non_empty() {
                let mut test_server = set_server();
                test_server.chain.link(FromFn::new(set_session_to_b));
                test_server.chain.link(FromFn::new(upsert_session));
                test_server.chain.link(FromFn::new(check_session_is_set_to_a));
                run_server(test_server);
            }
        }

        #[test]
        fn removes_session() {
            let mut test_server = set_server();
            test_server.chain.link(FromFn::new(set_session_to_a));
            test_server.chain.link(FromFn::new(check_session_is_set_to_a));
            test_server.chain.link(FromFn::new(remove_session));
            test_server.chain.link(FromFn::new(check_session_is_not_set));
            run_server(test_server);
        }
    }
}
