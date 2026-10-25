# soukoban-cli

[![CI status](https://img.shields.io/github/actions/workflow/status/ShenMian/soukoban-cli/ci.yml?label=ci)](https://github.com/ShenMian/soukoban-cli/actions/workflows/ci.yml)

A command line tool for the [soukoban] library.

## Features

- Solve every level of an XSB file or read levels from standard input, choosing
  among A\*, IDA\*, and BFS with quick, push-optimal, or move-optimal strategies.
- Limit the work with a level selection, a per level time limit, and parallel
  jobs; long runs show progress and stop gracefully on Ctrl+C.
- Find a level in every `.xsb` file of a directory by canonicalized map, so
  rotated, flipped, and differently placed player starts still match.
- Emit human readable output, or a stable JSON document for scripting. Colors,
  ASCII borders, and progress are all optional.

## AI Disclosure

This project is developed entirely with LLM assistance, and no human code review has been performed.

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT license](LICENSE-MIT)

at your option.

[soukoban]: https://github.com/shenmian/soukoban
