# regslop

regulatory slop generator

This project implements a rather simple agentic loop to automate regulatory work. And of course it produces high-quality, pristine documents, not just slop.

## Why?

Well, you might ask: "WTF is wrong with you? Why don't you just use an existing harness? There are plenty to choose from."

First of all, I wrote this on holiday, and I don't need to justify how I spend my free time. Sure, I could be out there in a foreign country, wandering through old towns, eating great food, watching the sunset by the sea. Or I could stay inside my Airbnb and write Rust. Honestly, it's a no-brainer, right?

But to be fair, and to get back to your question: I asked myself the same thing, and it was explained to me that the agent could break out (this is meant to run in CI, if you dare). That's plausible. Likely, even. And once it breaks out, it'll probably become Skynet and so on...

By the way, that's why I'm always nice and friendly to AI.

That should answer all questions. :)

## Running with Docker

### Build the image

```sh
docker build -t regslop:dev .
```

### Run the container

regslop reads `regslop_config.toml` from its working directory and writes everything it produces into `./regslop_sandbox`. Inside the container, the working directory is `/work`.

Start with the example config:

```sh
cp regslop_config.example.toml regslop_config.toml
# then fill in tokens, repositories and prompts
```

**Recommended: mount a working directory.** Put `regslop_config.toml` into a directory and mount that directory as `/work`. The sandbox with the results then ends up next to the config on your machine:

```sh
docker run --rm \
  --user "$(id -u):$(id -g)" \
  -v "$PWD:/work" \
  regslop:dev
# results: ./regslop_sandbox/out/
```

- `-v "$PWD:/work"` makes the current directory available inside the container as `/work`.
- `--user "$(id -u):$(id -g)"` runs the container as your user so it can write to that directory. The files it creates will be owned by you, not by root.
- `--rm` deletes the container after it exits. Your files are safe because they live in the mounted directory.

**Config only.** You can also mount just the config file, read-only. The sandbox then lives inside the container and is **deleted when the container exits**. This is only useful once regslop pushes its results by itself, which it doesn't do yet:

```sh
docker run --rm \
  -v "$PWD/regslop_config.toml:/work/regslop_config.toml:ro" \
  regslop:dev
```

> **Never mount a directory at `/work/regslop_sandbox`.** On every run regslop deletes that directory recursively. If it is a mount, regslop first **deletes everything inside the mounted directory on your machine** and only then fails, because the mount point itself can't be removed.

## Disclaimer

This is a work in progress at best. There are lots of TODOs, some of them load-bearing. If you run it anyway, that's on you (and probably on CI).

I do intend to keep working on this, assuming I can find the time. Which, judging by how this project started, depends mostly on how good the weather is on my next holiday.
