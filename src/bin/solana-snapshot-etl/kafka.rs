use {
    anyhow::Context,
    futures::future::try_join_all,
    indicatif::ProgressBar,
    rdkafka::{
        config::ClientConfig,
        producer::{FutureProducer, FutureRecord},
    },
    serde::Deserialize,
    sha2::{Digest, Sha256},
    solana_snapshot_etl::{append_vec::AppendVec, append_vec_iter, parallel::AppendVecConsumer},
    std::{collections::HashMap, path::Path, sync::Arc},
    tokio::{fs, sync::Semaphore},
    yellowstone_grpc_geyser::{
        config::ConfigGrpcFilters,
        filters::Filter,
        grpc::{Message, MessageAccount, MessageAccountInfo},
    },
    yellowstone_grpc_kafka::config::{ConfigGrpc2KafkaRequest, GrpcRequestToProto},
    yellowstone_grpc_proto::prost::Message as _,
};

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub filter: ConfigGrpc2KafkaRequest,
    pub kafka: HashMap<String, String>,
    pub kafka_topic: String,
    #[serde(default = "Config::kafka_queue_size_default")]
    pub kafka_queue_size: usize,
}

impl Config {
    const fn kafka_queue_size_default() -> usize {
        10_000
    }

    pub async fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data = fs::read(path).await?;
        serde_json::from_slice(&data).map_err(Into::into)
    }
}

#[derive(Clone)]
pub struct KafkaConsumer {
    pub filter: Filter,
    pub kafka: FutureProducer,
    pub kafka_topic: String,
    pub kafka_permits: Arc<Semaphore>,
    pub progress_bar: Arc<ProgressBar>,
}

impl KafkaConsumer {
    pub async fn new(
        config_path: impl AsRef<Path>,
        progress_bar: Arc<ProgressBar>,
    ) -> anyhow::Result<Self> {
        let config = Config::load(config_path).await?;

        let mut kafka_config = ClientConfig::new();
        for (key, value) in config.kafka.iter() {
            kafka_config.set(key, value);
        }
        let kafka = kafka_config
            .create()
            .context("failed to create kafka producer")?;

        Ok(Self {
            filter: Filter::new(&config.filter.to_proto(), &ConfigGrpcFilters::default())?,
            kafka,
            kafka_topic: config.kafka_topic,
            kafka_permits: Arc::new(Semaphore::new(config.kafka_queue_size)),
            progress_bar,
        })
    }
}

#[async_trait::async_trait]
impl AppendVecConsumer for KafkaConsumer {
    async fn on_append_vec(&mut self, append_vec: AppendVec) -> anyhow::Result<()> {
        let slot = append_vec.slot();

        let mut count = 0;
        let mut handlers = vec![];
        for account in append_vec_iter(&append_vec) {
            if let Some(account) = account.access() {
                let message = Message::Account(MessageAccount {
                    account: MessageAccountInfo {
                        pubkey: account.meta.pubkey,
                        lamports: account.account_meta.lamports,
                        owner: account.account_meta.owner,
                        executable: account.account_meta.executable,
                        rent_epoch: account.account_meta.rent_epoch,
                        data: account.data.to_vec(),
                        write_version: account.meta.write_version_obsolete,
                        txn_signature: None,
                    },
                    slot,
                    is_startup: true,
                });

                for message in self.filter.get_update(&message) {
                    let payload = message.encode_to_vec();
                    let hash = Sha256::digest(&payload);
                    let key = format!("{slot}_{}", const_hex::encode(hash));

                    let record = FutureRecord::to(&self.kafka_topic)
                        .key(&key)
                        .payload(&payload);

                    match self.kafka.send_result(record) {
                        Ok(future) => {
                            let permits = Arc::clone(&self.kafka_permits);
                            handlers.push(tokio::spawn(async move {
                                let _permit = permits.acquire_owned().await?;
                                future.await?.map_err(|(error, _message)| error)?;
                                Ok::<(), anyhow::Error>(())
                            }));
                        }
                        Err(error) => return Err(error.0.into()),
                    }
                }
            }

            count += 1;
        }
        try_join_all(handlers).await?;
        self.progress_bar.inc(count);

        Ok(())
    }
}
