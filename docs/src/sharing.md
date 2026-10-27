# Sharing a library

The library is plain files and the search index is a local cache, so a Git repository is all a group needs to share one: no database server. PDFs, figures and supplementary files go to [Git LFS](https://git-lfs.com), which GitHub, GitLab, Gitea and Forgejo all support. Keep the repository private: it holds publishers' PDFs.

## Setting it up

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

## Day to day

```sh
git pull                       # papers, PDFs and drafts from the others
carrel add 10.1029/2010JE003711
git add -A && git commit -m "add hiesinger2011" && git push
```

Commit everything in the library. The extracted text (`fulltext.md`) and figures are committed too, so nobody has to extract them again. The search index stays out of it, on each machine in `~/.cache/carrel`, and carrel updates it for every paper a pull changed.

Drafts sync as well: an AI on a server can propose summaries and keywords that someone approves on their laptop.

## Conflicts

Each paper is its own folder, so two people adding papers never conflict. `merge=union` lets two people change the same project at once: Git keeps the lines of both. A conflict needs two people editing the same paper's `.bib`; resolve it like any other, then check with `carrel doctor`.

Union merges can keep a paper that one person removed from a project while the other changed the lines right next to it. `carrel project show` makes that easy to spot.
