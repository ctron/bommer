# bommer – BOMs pods

[![CI](https://github.com/xkcd-2347/bommer/workflows/CI/badge.svg)](https://github.com/xkcd-2347/bommer/actions?query=workflow%3A%22CI%22)
[![GitHub release (latest SemVer)](https://img.shields.io/github/v/tag/xkcd-2347/bommer?sort=semver)](https://github.com/xkcd-2347/bommer/releases)

*WIP* This is in an experimental state.

## Running

You will need an instance of [bombastic](https://github.com/xkcd-2347) running. If it's not running on `localhost:8080`,
you also need to set the URL using the environment variable `BOMBASTIC_URL`.

```shell
env BIND_ADDR="[::]:8010" cargo run
```
