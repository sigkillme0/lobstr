# lobstr

browse [lobste.rs](https://lobste.rs/) from your terminal.

## install

```sh
cargo build --release
# binary is at ./target/release/lobstr
```

## what you can do

browse the front page, newest stories, or active threads:

```sh
lobstr hot
lobstr new
lobstr active
```

filter by tag, combine tags, exclude what you don't want:

```sh
lobstr tag rust
lobstr tag rust,web --any
lobstr tag linux --exclude satire
```

search stories or comments:

```sh
lobstr search "memory safety"
lobstr search sqlite -w comments -o newest
```

see a story and its comments — the short id is the thing in brackets next to each title:

```sh
lobstr view ngwloq
lobstr view ngwloq --full --sort score
```

read the actual article without leaving your terminal. works with blog posts, github repos (shows the README), and youtube videos (shows the transcript):

```sh
lobstr open ngwloq
lobstr open ngwloq --full -w 60
```

look up users:

```sh
lobstr user info pushcx
lobstr user stories pushcx
lobstr user comments pushcx
```

every listing command takes `-l` for limit, `-p` for page, `-s` for minimum score, and `--after`/`--before` for date ranges:

```sh
lobstr hot -l 10 -s 20
lobstr new --after 1d
lobstr tag rust --after 1w --before 3d
lobstr hot --after 2024-01-01 --before 2024-02-01
```

dates can be relative (`1h`, `1d`, `1w`, `1m`, `1y`) or absolute (`2024-01-15`).

## output formats

everything defaults to colored terminal output. pass `--json` for json, or `-F` for other formats:

```sh
lobstr hot --json
lobstr hot -F tsv
lobstr hot -F ids
```

the `ids` format is one short id per line, which makes piping easy:

```sh
lobstr hot -F ids | xargs -I{} lobstr open {} --json > articles.json
lobstr hot -F tsv | awk -F'\t' '$3 >= 20 {print $2}'
```

`--strict` makes the exit code 1 when there are no results, which is handy for scripts.

## config

you can drop a config file at `~/.config/lobstr/config.toml` if you want different defaults:

```toml
default_limit = 15
color = false
```

cli flags always win over config. color respects `NO_COLOR` by default.

## license

MIT
