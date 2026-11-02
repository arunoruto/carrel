---
title: "Recommendations"
weight: 2
---

Google Scholar emails you papers it thinks you'd like: *Recommended articles*, new citations of your papers, new papers by people you follow. carrel reads those alert emails and keeps their papers in a list of recommendations, outside the library, until you add or dismiss each. Search, the map and your bibliographies only ever see papers you chose. With [semantic search](/docs/guide/semantic-search.md) set up, each recommendation is scored by how alike it is to your library, so the best fits come first.

```text
$ carrel recommended
0.67  Bi-directional reflectance, polarization, and spectroscopic measurements of the Chang'e-5 regolith sample
      T Jiang et al. · 2025  [bi-directional-reflectance-polarization-and-spectroscopic]
0.64  On Error Estimates for Hapke Photometric Model Parameters from Disk-integrated Phase Curves of Airless Planetary Bodies
      P Helfenstein et al. · The Planetary Science Journal, 2025, PDF, recommended 2×  [on-error-estimates-for-hapke-photometric-model-parameters]
```

The score is the mean cosine similarity to the three most alike papers in the library. With bge-m3, a planetary-science library scores its Scholar recommendations between about 0.45 and 0.7: above 0.6 is close to what you have, below 0.5 mostly another field.

## Getting the emails to carrel

Scholar has no API, but its alerts are ordinary emails. Save them as `.eml` files into a folder, and point carrel at it:

```sh
carrel recommended import ~/Documents/scholar-alerts   # a folder of .eml files, or single files
```

Reading the same email twice changes nothing; a paper recommended again by a later alert counts as recommended twice. Other emails in the folder are skipped.

To keep it going by itself, add the folder to the ones [`carrel consume`](/docs/guide/adding-papers.md#from-your-downloads) looks in. Every run then reads the alerts there, and when it runs unattended, the [notification](/docs/reference/configuration.md#notifications) lists the new recommendations:

```toml
[consume]
folders = ["~/Downloads", "~/Documents/scholar-alerts"]
```

With Home Manager, the same goes into `services.carrel.consume.folders`, which then also watches the folder for new files.

### Saving alerts with Thunderbird

The [FiltaQuilla](https://addons.thunderbird.net/thunderbird/addon/filtaquilla/) add-on gives Thunderbird's message filters a *Save Message as File* action:

1. In FiltaQuilla's settings, enable **Save Message as File**, then restart Thunderbird.
2. In Tools › Message Filters, make a new filter for the account the alerts arrive in: run it when getting new mail, match *From* contains `scholaralerts-noreply@google.com`, and as action *Save Message as File* into your alerts folder.
3. To read the alerts you already have, select the filter, choose the folder they're in (for Gmail, *All Mail*) and click **Run Now**.

Don't let the same filter move the message to another folder; use FiltaQuilla's *Move Later* for that. Other mail clients work as well, as long as something saves the alerts as `.eml` files: most can save a message that way by hand.

## Adding and dismissing

```sh
carrel recommended add <key>       # into the library
carrel recommended dismiss <key>   # never shown again
carrel recommended --all           # with the dismissed ones
```

The TUI's **Discover** tab and the web UI's **Discover** page show the same list: `a` adds, `d` dismisses, `o` opens the paper's page.

Adding looks the paper up by the arXiv id, ADS bibcode or DOI in the alert's link (a DOI only when its title is this paper's), else by its exact title. When the alert links a PDF (`[PDF]` in the email), carrel downloads it, and arXiv papers come with theirs anyway. Many such links point to ResearchGate or a publisher that wants a login; then the paper is added without its PDF and `carrel attach` takes one later.

### Adding the best ones by themselves

To let the library grow on its own, add recommendations above a score without asking:

```toml
[consume]
recommended = 0.62
```

Each unattended `carrel consume` then adds new recommendations at least that alike to the library, and the notification says which. It needs [embeddings](/docs/guide/semantic-search.md); without them nothing is scored, so nothing is added by itself.

`carrel recommended score` scores every recommendation again, for example after setting up embeddings or after the library grew.

## Where they are

Each recommendation is a JSON file in `recommended/` in the library: its title, the authors and venue as Scholar writes them (shortened, with `…`), the start of its abstract, the link, the days it was recommended and its score. They travel with the library when you [share it](/docs/guide/sharing.md), so a dismissal on one machine holds on the others. Your AI sees them through the `recommended` [MCP tool](/docs/reference/mcp-tools.md).
