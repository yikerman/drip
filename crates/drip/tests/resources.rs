use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use drip::resource::Resources;

#[test]
fn snapshots_share_first_load_and_keep_old_values_until_dropped() {
    let path = Path::new("shared.raw");
    let mut resources = Resources::default();
    let snapshot = resources.snapshot([path]);
    let loads = AtomicUsize::new(0);
    let load = || {
        loads.fetch_add(1, Ordering::SeqCst);
        Ok::<_, String>(vec![42])
    };
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(|| resources.load(path, |_| load()).unwrap());
        let b = s.spawn(|| snapshot.load(path, |_| load()).unwrap());
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&a, &b));
    let old = Arc::downgrade(&a);
    drop((a, b));

    resources.reload(path);
    assert_eq!((resources.revision(path), snapshot.revision(path)), (1, 0));
    assert_eq!(*resources.load(path, |_| Ok(vec![84])).unwrap(), [84]);
    assert_eq!(*snapshot.load::<Vec<i32>>(path, |_| panic!("already loaded")).unwrap(), [42]);
    assert!(old.upgrade().is_some());
    drop(snapshot);
    assert!(old.upgrade().is_none(), "reload releases the old value after its last snapshot");
}
