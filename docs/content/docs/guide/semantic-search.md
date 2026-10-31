---
title: "Semantic search"
weight: 6
aliases: ["/semantic-search.html"]
---

`carrel search` finds the words you type. `carrel search -s` finds papers that *mean* what you type, in other words or other languages:

```sh
carrel search -s "how bright the moon looks depending on the angle of the sun"
carrel search -s "Mondoberfläche Rauigkeit" project:thesis
```

Each result shows how close it is (cosine similarity, up to 1) and the passage that matched, with where it starts in the full text. AIs get the same through the `search` tool's `semantic` option.

## Setting it up

Semantic search needs an embedding model on [Ollama](https://ollama.com), on your machine or another one:

```toml
[embeddings]
ollama = "http://boba.local:11434"   # http://localhost:11434 by default; OLLAMA_HOST overrides it
# model = "bge-m3"
```

If Ollama's machine is a `.local` name and embedding fails after a while with "Device or resource busy", give its IP address instead: mDNS lookups can stop working in a long run.

Without `model`, carrel uses bge-m3 if Ollama has it (multilingual, and good with long text), else the first embedding model Ollama has. `ollama pull bge-m3` on the Ollama machine gets it. `carrel config` says which model it found, or what's wrong: Ollama unreachable, no embedding model, or a configured model that's missing or can't embed.

## Computing the embeddings

```sh
carrel embed --all     # every paper whose embeddings are missing or out of date
carrel embed <id>…
```

carrel cuts each paper into passages: the head (title, authors, keywords, abstract, summary), then the full text in pieces of about 1500 characters, cut at paragraphs. Each passage becomes a vector, and the vectors go into the paper's folder as `embeddings.json`, together with the model that made them and a hash of the text. So they travel with the library: computed once, by whoever runs `carrel embed` or by the [server](/docs/guide/server.md), and everyone else gets them with `carrel sync`.

Embeddings go out of date when a paper's text changes or the model does; `carrel embed --all` redoes just those. Searching only compares vectors of the same model, so after switching models, papers not yet redone are missed.

A [server](/docs/guide/server.md) with `[embeddings]` in its config embeds new papers in its rounds. When it can't reach Ollama, it says so once in its log and through the [notifiers](/docs/reference/configuration.md#notifications), and again when Ollama works again.
