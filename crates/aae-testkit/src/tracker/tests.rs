use super::*;

#[tokio::test]
async fn a_key_creates_one_bead_whose_parent_is_released_while_a_child_is_open() {
    let t = MemoryTracker::new();
    let root = t.create("aae:r1", "k1", "Ship it", "text").await.unwrap();
    assert_eq!(t.create("aae:r1", "k1", "Other", "").await.unwrap(), root);
    assert_ne!(t.create("aae:r1", "k2", "Two", "").await.unwrap().id, root.id);
    let child = t.add(&[], Some(&root.id));
    t.claim("aae:r1", &root.id).await.unwrap();
    assert!(t.ready().await.unwrap().iter().any(|b| b.id == child));
    t.close("aae:r1", &root.id, "passed").await.unwrap();
    let root = t.bead(&root.id).unwrap();
    assert_eq!((root.status.as_str(), root.assignee), ("open", None));
    assert!(t.ready().await.unwrap().iter().all(|b| b.id != root.id));
}

#[tokio::test]
async fn under_gives_the_ready_beads_at_any_depth_below_the_root() {
    let t = MemoryTracker::new();
    let (root, other) = (t.add(&[], None), t.add(&[], None));
    let epic = t.add(&[], Some(&root));
    let (kid, outside) = (t.add(&[], Some(&epic)), t.add(&[], Some(&other)));
    assert_eq!(t.under(&root).await.unwrap(), [kid]);
    assert_eq!(t.under(&other).await.unwrap(), [outside]);
    assert_eq!(t.under("b-404").await, Err(TrackerError::NotFound));
}
