# Solana Snapshot ETL 📸

[![crates.io](https://img.shields.io/crates/v/solana-snapshot-etl?style=flat-square&logo=rust&color=blue)](https://crates.io/crates/solana-snapshot-etl)
[![docs.rs](https://img.shields.io/badge/docs.rs-solana--snapshot--etl-blue?style=flat-square&logo=docs.rs)](https://docs.rs/solana-snapshot-etl)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](#license)

**`solana-snapshot-etl` efficiently extracts all accounts in a snapshot** to load them into an external system.

## Motivation

Solana nodes periodically backup their account database into a `.tar.zst` "snapshot" stream.
If you run a node yourself, you've probably seen a snapshot file such as this one already:

```
snapshot-139240745-D17vR2iksG5RoLMfTX7i5NwSsr4VpbybuX1eqzesQfu2.tar.zst
```

A full snapshot file contains a copy of all accounts at a specific slot state (in this case slot `139240745`).

Historical accounts data is relevant to blockchain analytics use-cases and event tracing.
Despite archives being readily available, the ecosystem was missing an easy-to-use tool to access snapshot data.

## Building

```shell
cargo install --git https://github.com/rpcpool/solana-snapshot-etl --bins
```

## Usage

The ETL tool can extract snapshots from a variety of streaming sources
and load them into one of the supported storage backends.

The basic command-line usage is as follows:

```
$ solana-snapshot-etl --help
Efficiently unpack Solana snapshots

Usage: solana-snapshot-etl --source <SOURCE> <COMMAND>

Commands:
  noop   Load accounts and do nothing
  kafka  Filter accounts with gRPC plugin filter and send them to Kafka
  help   Print this message or the help of the given subcommand(s)

Options:
      --source <SOURCE>  Snapshot source (unpacked snapshot, archive file, or HTTP link)
  -h, --help             Print help
  -V, --version          Print version
```

### Sources

Extract from a local snapshot file:

```shell
solana-snapshot-etl --source /path/to/snapshot-*.tar.zst noop
```

Extract from an unpacked snapshot:

```shell
# Example unarchive command
tar -I zstd -xvf snapshot-*.tar.zst ./unpacked_snapshot/

solana-snapshot-etl --source ./unpacked_snapshot/ noop
```

Stream snapshot from HTTP source or S3 bucket:

```shell
solana-snapshot-etl 'https://my-solana-node.bdnodes.net/snapshot.tar.zst?auth=xxx' noop
```

### Targets

#### noop

Do nothing, only load snapshot, parse accounts.

#### kafka

```shell
solana-snapshot-etl --source /path/to/snapshot-*.tar.zst kafka --config kafka-config.json
```

Load snapshot, parse account, filter with [Solana Geyser gRPC Plugin](https://github.com/rpcpool/yellowstone-grpc)
filter and send filtered accounts to Kafka.
