---
title: "Sharing a library"
weight: 11
aliases: ["/sharing.html"]
---

The library is plain files and the search index is a local cache, so sharing it needs no database server. `carrel sync` shares it through an S3 bucket ([Garage](https://garagehq.deuxfleurs.fr), MinIO, AWS, ...) or a plain folder; [Git](#with-git) works as well. Keep whichever you use private: it holds publishers' PDFs.

## Through a bucket

Each member adds the bucket to their config:

```toml
# ~/.config/carrel/config.toml
[remote]
type = "s3"
endpoint = "https://s3.example.org"
region = "garage"                    # what the server calls its region
bucket = "papers"
access_key = "GK..."
secret_key = { file = "~/.config/carrel/s3-secret" }
# prefix = "group-a"                 # a folder in the bucket, to keep several libraries in one
```

The secret can also be written out (`secret_key = "..."`), and the usual `AWS_ENDPOINT_URL`, `AWS_REGION`, `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` override the config. A folder (a network drive, a USB stick) works the same way:

```toml
[remote]
type = "folder"
path = "/mnt/share/papers"
```

Then, whenever you like:

```sh
carrel sync -n     # what would happen
carrel sync        # do it
```

A sync uploads what changed here and downloads what changed on the remote: papers, PDFs, extracted text and figures, notes, drafts, projects and confirmed people. Hidden files and folders stay local, and so does the search index, which carrel updates for every paper a sync brought in. Eight files go at a time; an interrupted sync picks up where it stopped.

carrel remembers what both sides held after each sync, in `.carrel/sync.json` in the library, so it can tell who changed what:

- a file changed on one side is copied to the other;
- a file deleted on one side is deleted on the other, unless the other side changed it meanwhile: an edit beats a deletion;
- a file changed on both sides is a conflict and left alone until `carrel sync --keep local` or `--keep remote` settles it.

Nothing carrel deletes or overwrites locally is lost: it goes to `.carrel/trash`, keeping its path, and you can empty that folder whenever you like.

A few things carrel refuses:

- a remote that is suddenly empty while files here were synced with it before. That is more likely a wrong bucket or prefix, or a wiped remote, than everyone deleting everything, so carrel stops instead of moving the whole library to the trash. If the remote really is meant to be empty, delete the files here yourself; to fill it again from here, delete `.carrel/sync.json`;
- names on the remote that aren't plain paths inside the library (`../x`, `/x`, `a//b`, folder markers like `papers/` that some S3 tools create): skipped, with a warning. Hidden ones (`.carrel/...`) are skipped quietly, as hidden files here are;
- two syncs of one library at once (say `carrel sync` while `carrel serve` syncs): the second stops with "another sync is running".

## Sharing with your group

A shared library is one bucket that everyone syncs with. carrel has no accounts: whoever holds a key to the bucket can read it, and with write access change and delete anything in it.

**A key per person.** Give each member a key of their own rather than passing one around, so one can be taken back without changing everyone's config. With [Garage](https://garagehq.deuxfleurs.fr):

```sh
garage key create carrel-alice
garage bucket allow --read --write papers --key carrel-alice   # without --write, alice can only read
```

It prints the key's ID and secret once; they go in alice's `[remote]` as `access_key` and `secret_key`. Her first `carrel sync` downloads the whole library.

**Syncing by itself.** The longer people go between syncs, the more their edits cross. A service syncs every 10 minutes and at login:

```sh
carrel service install sync     # Home Manager: services.carrel.sync.enable = true;
```

**Conflicts.** A file changed by two people between syncs is a conflict. `carrel sync` by hand leaves it alone and says so, until `--keep local` or `--keep remote` settles it. The service can't wait for anyone, so it keeps the shared version, which everyone else already has, and saves yours in `.carrel/conflicts/<time>/` in the library, keeping its path. It tells you through your [notifiers](/docs/reference/configuration.md#notifications) about the files someone wrote, naming them: metadata, notes, summaries, drafts, projects. Text, figures, embeddings and references are made again from the PDF, so their conflicts are settled without a message. To bring your edit back, compare the two and copy what you want over the shared file; the next sync shares it.

Most files belong to one paper and change rarely, so conflicts are rare in practice: two people editing the same paper's notes, metadata or summary within minutes, or re-extracting the same PDF.

**Everything is shared**, including what isn't a paper: [recommendations](/docs/guide/recommendations.md) (one person's dismiss hides it for everyone), drafts, projects and confirmed people. So are deletions: `carrel rm` removes the paper for all. Only the trash on each machine that synced keeps a copy, so back up the bucket itself now and then, for instance with a nightly `rclone sync` to another disk.

## With Git

Git keeps history, at the price of committing by hand. PDFs, figures and supplementary files go to [Git LFS](https://git-lfs.com), which GitHub, GitLab, Gitea and Forgejo all support. Don't sync the same library through Git and a bucket.

### Setting it up

Once, in the library (`carrel config` shows where it is):

```sh
cd ~/Documents/carrel
git init
git lfs install
cat > .gitattributes <<'ATTR'
*.pdf              filter=lfs diff=lfs merge=lfs -text
*.png              filter=lfs diff=lfs merge=lfs -text
*/supplementary/** filter=lfs diff=lfs merge=lfs -text
projects/*.txt     merge=union
ATTR
git add -A
git commit -m "carrel library"
git remote add origin git@git.example.org:group/papers.git
git push -u origin main
```

Everyone else clones it and points carrel at the clone:

```sh
git lfs install
git clone git@git.example.org:group/papers.git ~/Documents/papers
```

```toml
# ~/.config/carrel/config.toml
library = "~/Documents/papers"
```

### Day to day

```sh
git pull                       # papers, PDFs and drafts from the others
carrel add 10.1029/2010JE003711
git add -A && git commit -m "add hiesinger2011" && git push
```

Commit everything in the library. The extracted text (`fulltext.md`) and figures are committed too, so nobody has to extract them again. The search index stays out of it, on each machine in `~/.cache/carrel`, and carrel updates it for every paper a pull changed.

Drafts sync as well: an AI on a server can propose summaries and keywords that someone approves on their laptop.

### Conflicts

Each paper is its own folder, so two people adding papers never conflict. `merge=union` lets two people change the same project at once: Git keeps the lines of both. A conflict needs two people editing the same paper's `.bib`; resolve it like any other, then check with `carrel doctor`.

Union merges can keep a paper that one person removed from a project while the other changed the lines right next to it. `carrel project show` makes that easy to spot.
