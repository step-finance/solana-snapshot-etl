use {
    crate::{AppendVec, AppendVecIterator},
    crossbeam::sync::WaitGroup,
};

pub trait AppendVecConsumer {
    fn on_append_vec(&mut self, append_vec: AppendVec) -> anyhow::Result<()>;
}

pub fn par_iter_append_vecs<F, A>(
    iterator: AppendVecIterator<'_>,
    create_consumer: F,
    num_threads: usize,
) -> anyhow::Result<()>
where
    F: Fn() -> A,
    A: AppendVecConsumer + Send + 'static,
{
    let (tx, rx) = crossbeam::channel::bounded::<AppendVec>(num_threads * 2);
    let wg = WaitGroup::new();

    for _ in 0..num_threads {
        let mut consumer = create_consumer();
        let rx = rx.clone();
        let wg = wg.clone();
        std::thread::spawn(move || {
            while let Ok(item) = rx.recv() {
                consumer.on_append_vec(item).expect("insert failed")
            }
            drop(wg);
        });
    }

    for append_vec in iterator {
        tx.send(append_vec?).expect("failed to send AppendVec");
    }

    drop(tx);
    wg.wait();

    Ok(())
}
