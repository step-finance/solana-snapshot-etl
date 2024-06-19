use {
    clap::{Parser, Subcommand},
    indicatif::{ProgressBar, ProgressBarIter, ProgressDrawTarget, ProgressStyle},
    log::info,
    reqwest::blocking::Response,
    solana_snapshot_etl::{
        append_vec::AppendVec,
        append_vec_iter,
        archived::ArchiveSnapshotExtractor,
        parallel::{par_iter_append_vecs, AppendVecConsumer},
        unpacked::UnpackedSnapshotExtractor,
        AppendVecIterator, ReadProgressTracking, SnapshotError, SnapshotExtractor, SnapshotResult,
    },
    std::{
        fs::File,
        io::{IoSliceMut, Read},
        path::Path,
        sync::Arc,
    },
};

#[derive(Debug, Parser)]
#[clap(author, version, about)]
struct Args {
    /// Snapshot source (unpacked snapshot, archive file, or HTTP link)
    #[clap(long)]
    source: String,

    /// Number of threads used to process snapshot,
    /// by default number of CPUs would be used.
    #[clap(long)]
    num_threads: Option<usize>,

    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Load accounts and do nothing
    Noop,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let args = Args::parse();
    let num_threads = args.num_threads.unwrap_or_else(num_cpus::get);

    let mut loader = SupportedLoader::new(&args.source, Box::new(LoadProgressTracking {}))?;
    let bar = Arc::new(create_accounts_progress_bar()?);
    match args.action {
        Action::Noop => {
            par_iter_append_vecs(
                loader.iter(),
                || NoopConsumer {
                    bar: Arc::clone(&bar),
                },
                num_threads,
            )
            .await?;
        }
    }
    bar.finish();
    info!("Done!");

    Ok(())
}

struct LoadProgressTracking {}

impl ReadProgressTracking for LoadProgressTracking {
    fn new_read_progress_tracker(
        &self,
        _path: &Path,
        rd: Box<dyn Read>,
        file_len: u64,
    ) -> SnapshotResult<Box<dyn Read>> {
        let progress_bar = ProgressBar::new(file_len).with_style(
            ProgressStyle::with_template(
                "{prefix:>10.bold.dim} {spinner:.green} [{bar:.cyan/blue}] {bytes}/{total_bytes} ({percent}%)",
            )
            .map_err(|error| SnapshotError::ReadProgressTracking(error.to_string()))?
            .progress_chars("#>-"),
        );
        progress_bar.set_prefix("manifest");
        Ok(Box::new(LoadProgressTracker {
            rd: progress_bar.wrap_read(rd),
            progress_bar,
        }))
    }
}

struct LoadProgressTracker {
    progress_bar: ProgressBar,
    rd: ProgressBarIter<Box<dyn Read>>,
}

impl Drop for LoadProgressTracker {
    fn drop(&mut self) {
        self.progress_bar.finish()
    }
}

impl Read for LoadProgressTracker {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.rd.read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> std::io::Result<usize> {
        self.rd.read_vectored(bufs)
    }

    fn read_to_string(&mut self, buf: &mut String) -> std::io::Result<usize> {
        self.rd.read_to_string(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.rd.read_exact(buf)
    }
}

pub enum SupportedLoader {
    Unpacked(UnpackedSnapshotExtractor),
    ArchiveFile(ArchiveSnapshotExtractor<File>),
    ArchiveDownload(ArchiveSnapshotExtractor<Response>),
}

impl SupportedLoader {
    fn new(source: &str, progress_tracking: Box<dyn ReadProgressTracking>) -> anyhow::Result<Self> {
        if source.starts_with("http://") || source.starts_with("https://") {
            Self::new_download(source)
        } else {
            Self::new_file(source.as_ref(), progress_tracking).map_err(Into::into)
        }
    }

    fn new_download(url: &str) -> anyhow::Result<Self> {
        let resp = reqwest::blocking::get(url)?;
        let loader = ArchiveSnapshotExtractor::from_reader(resp)?;
        info!("Streaming snapshot from HTTP");
        Ok(Self::ArchiveDownload(loader))
    }

    fn new_file(
        path: &Path,
        progress_tracking: Box<dyn ReadProgressTracking>,
    ) -> solana_snapshot_etl::SnapshotResult<Self> {
        Ok(if path.is_dir() {
            info!("Reading unpacked snapshot");
            Self::Unpacked(UnpackedSnapshotExtractor::open(path, progress_tracking)?)
        } else {
            info!("Reading snapshot archive");
            Self::ArchiveFile(ArchiveSnapshotExtractor::open(path)?)
        })
    }
}

impl SnapshotExtractor for SupportedLoader {
    fn iter(&mut self) -> AppendVecIterator<'_> {
        match self {
            SupportedLoader::Unpacked(loader) => Box::new(loader.iter()),
            SupportedLoader::ArchiveFile(loader) => Box::new(loader.iter()),
            SupportedLoader::ArchiveDownload(loader) => Box::new(loader.iter()),
        }
    }
}

fn create_accounts_progress_bar() -> anyhow::Result<ProgressBar> {
    let tmpl = ProgressStyle::with_template("{prefix:>10.bold.dim} {spinner:.green} rate={per_sec} processed={human_pos} {elapsed_precise:.cyan}")?;
    let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr()).with_style(tmpl);
    bar.set_prefix("accounts");
    Ok(bar)
}

struct NoopConsumer {
    bar: Arc<ProgressBar>,
}

#[async_trait::async_trait]
impl AppendVecConsumer for NoopConsumer {
    async fn on_append_vec(&mut self, append_vec: AppendVec) -> anyhow::Result<()> {
        let count = append_vec_iter(&append_vec).count();
        self.bar.inc(count as u64);
        Ok(())
    }
}
