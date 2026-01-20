# lobstr

a no-nonsense terminal tool to interact with [lobste.rs](https://lobste.rs/).

## build

```sh
cargo build --release
```

binary lands in `./target/release/lobstr`

## usage

```
lobstr <COMMAND>

Commands:
  hot     show hottest stories
  new     show newest stories
  tag     show stories by tag
  active  show active/recent discussions
  view    view a specific story with comments
  open    read article content in terminal
  user    show user profile
  tags    list all available tags
  help    Print this message or the help of the given subcommand(s)
```

## examples

### browse hot stories

```sh
lobstr hot
lobstr hot -l 10  # limit to 10
```

### browse newest

```sh
lobstr new
lobstr new -l 15 -p 2  # page 2, 15 items
```

### filter by tag

```sh
lobstr tag rust
lobstr tag linux -l 20
lobstr tag rust,web --any        # match ANY tag (OR)
lobstr tag rust --exclude satire # exclude stories with tag
```

### view story with comments

```sh
lobstr view ngwloq  # use short_id shown in brackets
lobstr view ngwloq --full        # show full comments
lobstr view ngwloq --sort score  # sort comments by score
```

### read article in terminal

```sh
lobstr open ngwloq               # read article content
lobstr open ngwloq --full        # show full content (no truncation)
lobstr open ngwloq -w 60         # wrap at 60 columns
lobstr open ngwloq --raw         # raw text, no formatting
lobstr open ngwloq --json        # output as json
```

supported sources:
- **youtube videos** - fetches and displays video transcripts
- **github repos** - fetches README with markdown rendering
- **blog posts / articles** - extracts main content via readability
- **self posts** - shows description

gracefully handles unsupported content:
- pdfs (shows url)
- audio/video files (shows url)

### view user profile

```sh
lobstr user info pushcx          # show profile
lobstr user stories pushcx       # show user's stories
lobstr user comments pushcx      # show user's comments
```

### list all tags

```sh
lobstr tags
lobstr tags -c programming  # filter by category
```

### active discussions

```sh
lobstr active
```

## global options

```sh
--json    # output as json (works with all commands)
--strict  # exit code 1 if no results (for scripting)
```

## dependencies

- [llm_readability](https://crates.io/crates/llm_readability) - article extraction
- [yt-transcript-rs](https://crates.io/crates/yt-transcript-rs) - youtube transcripts
- [termimad](https://crates.io/crates/termimad) - terminal markdown rendering

## license

MIT