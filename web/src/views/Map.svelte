<script lang="ts">
  import { ArrowUpRight, Plus, X } from '@lucide/svelte';
  import {
    forceCollide, forceLink, forceManyBody, forceRadial, forceSimulation, forceX, forceY,
    type Simulation, type SimulationNodeDatum,
  } from 'd3-force';
  import { untrack } from 'svelte';
  import { addSource } from '../lib/actions';
  import { api, type LibraryMap, type MapNode } from '../lib/api';
  import Spinner from '../lib/components/Spinner.svelte';
  import { go, paperHref, router } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';

  // The library as a network: papers linked by citations, through the authors they share (diamond hubs) and the
  // institutions they come from (square hubs), and by how alike they are. Papers the library lacks but several of
  // its papers cite can join in as grey rings. Drawn on a canvas; d3-force lays it out.
  type Place = LibraryMap['institutions'][number];
  type Missing = NonNullable<LibraryMap['missing']>[number];
  // A hub stands for an institution (`place`) or an author (`person`), linked to their papers.
  // `ghost`: a paper missing from the library. `group`: which of `groups` the links put it in.
  type Node = MapNode & SimulationNodeDatum & { degree: number; place?: Place; person?: string; ghost?: Missing; group?: number };
  type Kind = 'cites' | 'missing' | 'authors' | 'institutions' | 'similar';
  // `bend`: how far a link curves off the straight line, so two papers linked several ways show each link.
  // `self`: for a citation, the authors the two papers share.
  type Edge = { source: Node; target: Node; kind: Kind; names?: string[]; score?: number; bend: number; self?: string[] };
  const hub = (n: Node) => !!(n.place || n.person);

  // Raw: d3 moves the nodes itself, and the selection must stay the very node object.
  let data = $state.raw<LibraryMap | null>(null);
  let error = $state('');
  let project = $state(router.params.get('project') ?? '');
  // Which links to draw, remembered in this browser; citations alone to begin with, all of them at once is a lot.
  const LINKS = 'carrel-map-links';
  let show = $state<Record<Kind, boolean>>({ cites: true, missing: false, authors: false, institutions: false, similar: false, ...stored() });
  function stored(): Partial<Record<Kind, boolean>> {
    try {
      return JSON.parse(localStorage.getItem(LINKS) ?? '{}');
    } catch {
      return {};
    }
  }
  $effect(() => {
    const value = JSON.stringify(show);
    try {
      localStorage.setItem(LINKS, value);
    } catch {
      // Private windows: just not remembered.
    }
  });
  let alike = $state(0.75);
  // Missing papers shown when at least this many of the papers shown cite them.
  let often = $state(4);
  // What the colours say, remembered too.
  const COLORS = {
    group: 'Group',
    central: 'Central here',
    bridges: 'Bridges between groups',
    type: 'Type',
    year: 'Year',
    citations: 'Citations',
    momentum: 'Citations a year',
    cited: 'Cited here',
    hindex: "Authors' h-index",
    read: 'Your notes',
  } as const;
  type ColorBy = keyof typeof COLORS;
  const MENU: [string, ColorBy[]][] = [
    ['The network', ['group', 'central', 'bridges']],
    ['The paper', ['type', 'year', 'citations', 'momentum', 'cited']],
    ['People and you', ['hindex', 'read']],
  ];
  // What each continuous colour means at its two ends, and where its numbers come from.
  const ENDS: Partial<Record<ColorBy, [string, string, string?]>> = {
    central: ['on the edge', 'central', 'PageRank over the citations between the papers shown'],
    bridges: ['inside a group', 'between groups', 'how many shortest paths between papers run through it'],
    citations: ['few', 'many', 'from OpenAlex'],
    momentum: ['few', 'many', 'citations per year since it came out, from OpenAlex'],
    cited: ['once', 'often', 'by the papers shown'],
    hindex: ['lower', 'higher', 'the authors shared by two or more papers, from OpenAlex'],
  };
  let colorBy = $state<ColorBy>(remembered());
  function remembered(): ColorBy {
    try {
      const value = localStorage.getItem('carrel-map-color') ?? '';
      return value in COLORS ? (value as ColorBy) : 'group';
    } catch {
      return 'group';
    }
  }
  $effect(() => {
    const value = colorBy;
    try {
      localStorage.setItem('carrel-map-color', value);
    } catch {
      // Not remembered, then.
    }
  });
  // Groups of papers the links shown tie together, each named by its most common keyword.
  let groups = $state.raw<{ name: string; size: number }[]>([]);
  // How many shortest paths between the nodes shown run through each one.
  let between = $state.raw(new Map<string, number>());
  let hideLonely = $state(true);
  // The papers no link shown reaches, by why.
  let alone = $state.raw<Node[]>([]);
  // Bumped to load the map again, after adding a missing paper.
  let loads = $state(0);
  let adding = $state(false);
  let selected = $state.raw<Node | null>(null);
  let tip = $state<{ x: number; y: number; html: string } | null>(null);

  let canvas: HTMLCanvasElement;
  let box: HTMLDivElement;
  let size = $state({ w: 800, h: 600 });
  let view = { x: 400, y: 300, k: 1 };
  let nodes: Node[] = [];
  let edges: Edge[] = [];
  let sim: Simulation<Node, undefined> | null = null;
  let hovered: Node | Edge | null = null;
  const byId = new Map<string, Node>();

  let loaded = '';
  $effect(() => {
    const p = project;
    void loads;
    // Another project starts afresh; the same one again keeps the map until the new one comes.
    if (p !== loaded) data = null;
    loaded = p;
    api.map(p || undefined).then((m) => ((data = m), (error = ''))).catch((e) => (error = e.message));
  });

  // (Re)build the network when the data or the choices change, keeping where papers already are.
  $effect(() => {
    if (!data) return;
    const d = data;
    const [s, threshold, lonely, times] = [{ ...show }, alike, hideLonely, often];
    const before = new Map(byId);
    const place = (p: Place): MapNode & { place: Place } => {
      const id = `institution:${p.ror ?? p.name}`;
      return { id, title: p.name, authors: '', year: null, venue: p.country, keywords: [], projects: [], type: '', place: p };
    };
    // Each author two or more shown papers share, with those papers.
    const people = new Map<string, Set<string>>();
    d.authors.forEach(([a, b, names]) => names.forEach((name) => people.set(name, new Set([...(people.get(name) ?? []), a, b]))));
    const person = (name: string): MapNode & { person: string } => ({ id: `author:${name}`, title: name, authors: '', year: null, venue: null, keywords: [], projects: [], type: '', person: name });
    const short = (names: string[] = []) => {
      const family = (n: string) => (n.includes(',') ? n.split(',')[0] : n.split(' ').at(-1)!).trim();
      return names.length > 2 ? `${family(names[0])} et al.` : names.map(family).join(' & ');
    };
    const ghost = (m: Missing): MapNode & { ghost: Missing } => ({ id: `missing:${m.doi ?? m.arxiv ?? m.title}`, title: m.title, authors: short(m.authors), year: m.year ?? null, venue: m.venue || null, keywords: [], projects: [], type: '', ghost: m });
    const missing = s.missing ? (d.missing ?? []).filter((m) => (m.cited_by?.length ?? 0) >= times).map(ghost) : [];
    const hubs = [...(s.institutions ? d.institutions.map(place) : []), ...(s.authors ? [...people.keys()].map(person) : []), ...missing];
    const all = [...d.nodes, ...hubs].map((n) => {
      const old = before.get(n.id);
      return Object.assign(old ?? { x: (Math.random() - 0.5) * 400, y: (Math.random() - 0.5) * 400 }, n, { degree: 0 }) as Node;
    });
    byId.clear();
    all.forEach((n) => byId.set(n.id, n));
    const list: Edge[] = [];
    const add = (a: string, b: string, kind: Kind, extra: Partial<Edge> = {}) => {
      const [source, target] = [byId.get(a), byId.get(b)];
      if (source && target) list.push({ source, target, kind, bend: 0, ...extra });
    };
    const pair = (a: string, b: string) => (a < b ? `${a}\n${b}` : `${b}\n${a}`);
    const coauthors = new Map(d.authors.map(([a, b, names]) => [pair(a, b), names]));
    if (s.cites) d.cites.forEach(([a, b]) => add(a, b, 'cites', { self: coauthors.get(pair(a, b)) }));
    missing.forEach((m) => m.ghost.cited_by?.forEach((id) => add(id, m.id, 'missing')));
    if (s.authors) people.forEach((ids, name) => ids.forEach((id) => add(id, `author:${name}`, 'authors')));
    if (s.institutions) d.institutions.forEach((p) => Object.entries(p.papers).forEach(([id, names]) => add(id, place(p).id, 'institutions', { names })));
    if (s.similar) d.similar.filter(([, , sc]) => sc >= threshold).forEach(([a, b, score]) => add(a, b, 'similar', { score }));
    // Papers linked more than one way get their links side by side, so a citation between co-authors stays visible.
    const pairs = new Map<string, Edge[]>();
    list.forEach((e) => pairs.set(pair(e.source.id, e.target.id), [...(pairs.get(pair(e.source.id, e.target.id)) ?? []), e]));
    pairs.forEach((group) => group.forEach((e, i) => (e.bend = (i - (group.length - 1) / 2) * 22)));
    // Citations last, so they are drawn on top.
    const order: Record<Kind, number> = { similar: 0, institutions: 1, authors: 2, missing: 3, cites: 4 };
    list.sort((a, b) => order[a.kind] - order[b.kind]);
    list.forEach((e) => (e.source.degree++, e.target.degree++));
    nodes = lonely ? all.filter((n) => n.degree > 0) : all;
    alone = all.filter((n) => n.degree === 0 && !hub(n) && !n.ghost);
    edges = list;
    groups = grouped(nodes, edges);
    between = betweenness(nodes, edges);
    untrack(() => selected && !nodes.includes(selected) && (selected = null));
    const warm = !!sim;
    sim?.stop();
    sim = forceSimulation(nodes)
      .force(
        'link',
        forceLink<Node, Edge>(edges)
          .distance((e) => ({ authors: 26, cites: 60, missing: 60, institutions: 32, similar: 50 })[e.kind])
          // People and places pull hard, citations (which tie nearly everything together) loosely: groups clump.
          .strength((e) => ({ authors: 0.8, cites: 0.12, missing: 0.12, institutions: 0.6, similar: 0.3 })[e.kind]),
      )
      .force('charge', forceManyBody().strength(-140).distanceMax(450))
      // Papers no link reaches go round the edge, so they don't look part of the network.
      .force('x', forceX<Node>(0).strength((n) => (n.degree ? 0.04 : 0)))
      .force('y', forceY<Node>(0).strength((n) => (n.degree ? 0.04 : 0)))
      .force('ring', forceRadial<Node>(80 + 30 * Math.sqrt(nodes.filter((n) => n.degree).length), 0, 0).strength((n) => (n.degree ? 0 : 0.5)))
      .force('collide', forceCollide<Node>().radius((n) => radius(n) + 3))
      .on('tick', draw);
    // The first layout is worked out at once, so the map appears settled; later changes animate gently.
    if (warm) sim.alpha(0.4);
    else {
      sim.stop().tick(300);
      // #/map?focus=<id>: open on that paper, selected and in the middle.
      const focus = untrack(() => router.params.get('focus'));
      const n = focus ? byId.get(focus) : undefined;
      if (n && nodes.includes(n)) {
        selected = n;
        view = { k: 1.4, x: size.w / 2 - n.x! * 1.4, y: size.h / 2 - n.y! * 1.4 };
      }
      draw();
    }
  });

  $effect(() => () => sim?.stop());

  // Brandes' algorithm, links unweighted: a breadth-first search from each node, counting shortest paths back.
  function betweenness(ns: Node[], es: Edge[]): Map<string, number> {
    const near = new Map<Node, Node[]>(ns.map((n) => [n, []]));
    es.forEach((e) => (near.get(e.source)?.push(e.target), near.get(e.target)?.push(e.source)));
    const score = new Map<Node, number>(ns.map((n) => [n, 0]));
    for (const s of ns) {
      const order: Node[] = [];
      const before = new Map<Node, Node[]>();
      const paths = new Map<Node, number>([[s, 1]]);
      const depth = new Map<Node, number>([[s, 0]]);
      for (let queue = [s], i = 0; i < queue.length; i++) {
        const v = queue[i];
        order.push(v);
        for (const w of near.get(v)!) {
          if (!depth.has(w)) (depth.set(w, depth.get(v)! + 1), queue.push(w));
          if (depth.get(w) === depth.get(v)! + 1) {
            paths.set(w, (paths.get(w) ?? 0) + paths.get(v)!);
            before.set(w, [...(before.get(w) ?? []), v]);
          }
        }
      }
      const share = new Map<Node, number>();
      for (const w of order.reverse()) {
        for (const v of before.get(w) ?? []) {
          share.set(v, (share.get(v) ?? 0) + (paths.get(v)! / paths.get(w)!) * (1 + (share.get(w) ?? 0)));
        }
        if (w !== s) score.set(w, score.get(w)! + (share.get(w) ?? 0));
      }
    }
    return new Map([...score].filter(([n]) => !hub(n) && !n.ghost).map(([n, v]) => [n.id, v]));
  }

  // Label propagation: each node takes the label most of its neighbours have, until none changes. Authors and
  // institutions shown as hubs, and missing papers, tie their papers together too. Sets each paper's `group`, biggest group first.
  function grouped(ns: Node[], es: Edge[]): { name: string; size: number }[] {
    const near = new Map<Node, Node[]>(ns.map((n) => [n, []]));
    es.forEach((e) => (near.get(e.source)?.push(e.target), near.get(e.target)?.push(e.source)));
    const label = new Map<Node, number>(ns.map((n, i) => [n, i]));
    const order = [...ns].sort((a, b) => (a.id < b.id ? -1 : 1));
    for (let round = 0, changed = true; changed && round < 30; round++) {
      changed = false;
      for (const n of order) {
        const votes = new Map<number, number>();
        near.get(n)!.forEach((m) => votes.set(label.get(m)!, (votes.get(label.get(m)!) ?? 0) + 1));
        let [best, most] = [label.get(n)!, votes.get(label.get(n)!) ?? 0];
        votes.forEach((count, l) => count > most && ([best, most] = [l, count]));
        if (best !== label.get(n)) (label.set(n, best), (changed = true));
      }
    }
    const members = new Map<number, Node[]>();
    ns.filter((n) => !hub(n) && !n.ghost).forEach((n) => members.set(label.get(n)!, [...(members.get(label.get(n)!) ?? []), n]));
    // Groups of two or more get a colour, as many as the palette has; the rest stay grey.
    const big = [...members.values()].filter((g) => g.length > 1).sort((a, b) => b.length - a.length).slice(0, palette.length);
    ns.forEach((n) => (n.group = undefined));
    const used = new Set<string>();
    return big.map((g, i) => {
      g.forEach((n) => (n.group = i));
      const counts = new Map<string, number>();
      g.forEach((n) => n.keywords.forEach((k) => counts.set(k.toLowerCase(), (counts.get(k.toLowerCase()) ?? 0) + 1)));
      const name = [...counts].filter(([k, c]) => c > 1 && !used.has(k)).sort((a, b) => b[1] - a[1])[0]?.[0];
      if (name) used.add(name);
      return { name: name ?? `group ${i + 1}`, size: g.length };
    });
  }

  const radius = (n: Node) => (hub(n) ? 5 + Math.sqrt(n.degree) * 2 : 4 + Math.sqrt(n.degree) * 1.8);
  const palette = ['#e8586e', '#f2a33a', '#4fb06d', '#4c7ef0', '#a974e8', '#2fb5c4', '#e077b8', '#b5b83a', '#d9773a', '#7c8bd9', '#3fa38f', '#c46b4f'];
  const TYPES: Record<string, [string, string]> = {
    journal: ['Journal', '#4c7ef0'],
    conference: ['Conference', '#f2a33a'],
    preprint: ['Preprint', '#e8586e'],
    book: ['Book', '#4fb06d'],
    thesis: ['Thesis', '#a974e8'],
  };
  // How often the papers shown cite each one.
  const cited = $derived.by(() => {
    const counts = new Map<string, number>();
    (data?.cites ?? []).forEach(([, b]) => counts.set(b, (counts.get(b) ?? 0) + 1));
    return counts;
  });
  // PageRank over the citations: cited by papers that are themselves much cited counts most.
  const rank = $derived.by(() => {
    const ids = (data?.nodes ?? []).map((n) => n.id);
    const out = new Map<string, string[]>(ids.map((id) => [id, []]));
    (data?.cites ?? []).forEach(([a, b]) => out.get(a)?.push(b));
    let r = new Map(ids.map((id) => [id, 1 / ids.length]));
    for (let round = 0; round < 40; round++) {
      const next = new Map(ids.map((id) => [id, 0.15 / ids.length]));
      let lost = 0;
      out.forEach((to, id) => (to.length ? to.forEach((b) => next.set(b, next.get(b)! + (0.85 * r.get(id)!) / to.length)) : (lost += r.get(id)!)));
      // Papers citing none of the others pass theirs to all.
      ids.forEach((id) => next.set(id, next.get(id)! + (0.85 * lost) / ids.length));
      r = next;
    }
    return r;
  });
  const thisYear = new Date().getFullYear();
  // The number a continuous colour shows for a node; null leaves it grey. Counts that span orders of magnitude
  // are compared by their logarithm.
  function measure(n: Node, by: ColorBy): number | null {
    // The h-index belongs to the authors, so it colours their diamonds alone.
    if (n.person) return by === 'hindex' ? (data?.people?.[n.person] ?? null) : null;
    if (hub(n) || n.ghost || by === 'hindex') return null;
    switch (by) {
      case 'year':
        return n.year;
      case 'citations':
        return n.cited_by == null ? null : Math.log1p(n.cited_by);
      case 'momentum':
        return n.cited_by == null || !n.year ? null : Math.log1p(n.cited_by / Math.max(1, thisYear - n.year + 1));
      case 'cited':
        return cited.get(n.id) || null;
      case 'central':
        return rank.get(n.id) ?? null;
      case 'bridges':
        return between.get(n.id) || null;
      default:
        return null;
    }
  }
  // The colours run over the middle 90% of the values, so a few outliers don't leave everything else alike.
  const range = $derived.by(() => {
    void [between, rank, cited];
    const by = colorBy;
    const values = by === 'hindex' ? Object.values(data?.people ?? {}) : (data?.nodes ?? []).map((n) => measure(n as Node, by)).filter((v): v is number => v != null);
    const sorted = [...values].sort((a, b) => a - b);
    const at = (q: number) => sorted[Math.round(q * (sorted.length - 1))];
    return sorted.length ? { lo: at(0.05), hi: at(0.95), known: values.length } : { lo: 0, hi: 1, known: 0 };
  });
  // The ends of the scale as numbers, for the legend.
  const shown = (v: number) => {
    const by = colorBy;
    const n = by === 'citations' || by === 'momentum' ? Math.expm1(v) : v;
    return by === 'central' || by === 'bridges' ? '' : n >= 1000 ? `${Math.round(n / 1000)}k` : `${Math.round(n)}`;
  };

  // Blue for little or early, through green, to orange for much or late.
  const scale = (t: number) => `hsl(${220 - Math.min(1, Math.max(0, t)) * 200} 70% 58%)`;

  const READ: Record<string, [string, string]> = { notes: ['your notes', '#4fb06d'], summary: ['a summary only', '#b5d98a'] };

  function color(n: Node): string {
    if (n.ghost) return css('--card');
    switch (colorBy) {
      case 'group':
        return n.group === undefined ? css('--faint') : palette[n.group];
      case 'type':
        return TYPES[n.type]?.[1] ?? css('--faint');
      case 'read':
        return READ[n.read ?? '']?.[1] ?? css('--faint');
      default: {
        const v = measure(n, colorBy);
        return v == null ? css('--faint') : scale((v - range.lo) / Math.max(1e-9, range.hi - range.lo));
      }
    }
  }

  let styles: CSSStyleDeclaration;
  const css = (name: string) => (styles ??= getComputedStyle(document.documentElement)).getPropertyValue(name).trim();

  let frame = 0;
  function draw() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      paint();
    });
  }

  function linked(n: Node) {
    return new Set(edges.flatMap((e) => (e.source === n ? [e.target] : e.target === n ? [e.source] : [])));
  }

  function paint() {
    if (!canvas) return;
    styles = getComputedStyle(document.documentElement);
    const ctx = canvas.getContext('2d')!;
    const dpr = devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size.w, size.h);
    ctx.save();
    ctx.translate(view.x, view.y);
    ctx.scale(view.k, view.k);
    const focus = selected ?? (hovered && 'degree' in hovered ? hovered : null);
    const near = focus ? linked(focus) : null;
    const lit = (n: Node) => !near || n === focus || near.has(n);
    const kinds: Record<Kind, string> = { cites: css('--accent'), missing: css('--faint'), authors: css('--warning'), institutions: css('--title'), similar: css('--ready') };
    for (const e of edges) {
      const on = !focus || e.source === focus || e.target === focus;
      const arrowed = e.kind === 'cites' || e.kind === 'missing';
      ctx.globalAlpha = e === hovered ? 1 : on ? (focus ? 0.9 : e.kind === 'similar' ? 0.7 : arrowed ? 0.5 : 0.35) : 0.06;
      ctx.strokeStyle = kinds[e.kind];
      const width = e.kind === 'similar' ? 1 + 6 * Math.max(0, (e.score ?? 0) - 0.6) : e.kind === 'cites' ? 1.5 : 1.2;
      ctx.lineWidth = (width / Math.sqrt(view.k)) * (e === hovered ? 2 : 1);
      ctx.setLineDash(e.kind === 'similar' || e.kind === 'missing' ? [4, 4] : []);
      const [ax, ay, cx, cy, bx, by] = curve(e);
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.quadraticCurveTo(cx, cy, bx, by);
      ctx.stroke();
      if (arrowed && (on || !focus)) {
        // Heads stay clear even where the lines are faint: the direction is the point of a citation.
        ctx.globalAlpha = Math.min(1, ctx.globalAlpha * 2);
        arrow(ctx, e);
      }
    }
    ctx.setLineDash([]);
    for (const n of nodes) {
      ctx.globalAlpha = lit(n) ? 1 : 0.15;
      const r = radius(n);
      ctx.beginPath();
      if (n.place) ctx.roundRect(n.x! - r, n.y! - r, 2 * r, 2 * r, r / 3);
      else if (n.person) {
        const d = r * 1.25;
        ctx.moveTo(n.x!, n.y! - d);
        ctx.lineTo(n.x! + d, n.y!);
        ctx.lineTo(n.x!, n.y! + d);
        ctx.lineTo(n.x! - d, n.y!);
        ctx.closePath();
      } else ctx.arc(n.x!, n.y!, r, 0, Math.PI * 2);
      // Hubs stay hollow, but for authors coloured by their h-index.
      ctx.fillStyle = hub(n) && measure(n, colorBy) == null ? css('--card') : color(n);
      ctx.fill();
      if (hub(n) || n.ghost) {
        ctx.lineWidth = 2 / view.k;
        ctx.strokeStyle = n.place ? kinds.institutions : n.person ? kinds.authors : kinds.missing;
        ctx.stroke();
      }
      if (n === focus || n === hovered) {
        ctx.lineWidth = 2.5 / view.k;
        ctx.strokeStyle = css('--foreground');
        ctx.stroke();
      }
    }
    // Labels when zoomed in, and for the paper in focus and its neighbours.
    ctx.globalAlpha = 1;
    ctx.fillStyle = css('--foreground');
    ctx.font = `${11 / view.k}px ui-sans-serif, system-ui, sans-serif`;
    ctx.textAlign = 'center';
    for (const n of nodes) {
      if (view.k > 1.8 || (near && lit(n)) || (hub(n) && n.degree >= 5 && view.k > 0.7)) {
        ctx.globalAlpha = n === focus ? 1 : 0.75;
        ctx.fillText(label(n), n.x!, n.y! + radius(n) + 12 / view.k);
      }
    }
    ctx.restore();
  }

  // A link as a quadratic curve: its ends and the control point, `bend` off the middle. The bend is measured
  // from the lower id's side, so links between the same two papers spread apart whichever way they point.
  function curve(e: Edge): [number, number, number, number, number, number] {
    const [a, b] = [e.source, e.target];
    const [mx, my] = [(a.x! + b.x!) / 2, (a.y! + b.y!) / 2];
    const [p, q] = a.id < b.id ? [a, b] : [b, a];
    const len = Math.hypot(q.x! - p.x!, q.y! - p.y!) || 1;
    const [nx, ny] = [-(q.y! - p.y!) / len, (q.x! - p.x!) / len];
    return [a.x!, a.y!, mx + nx * e.bend, my + ny * e.bend, b.x!, b.y!];
  }

  function arrow(ctx: CanvasRenderingContext2D, e: Edge) {
    const [, , cx, cy, tx, ty] = curve(e);
    const angle = Math.atan2(ty - cy, tx - cx);
    const r = radius(e.target) + 1.5 / view.k;
    const [x, y] = [tx - Math.cos(angle) * r, ty - Math.sin(angle) * r];
    const a = 11 / Math.sqrt(view.k);
    ctx.fillStyle = ctx.strokeStyle as string;
    ctx.beginPath();
    ctx.moveTo(x, y);
    ctx.lineTo(x - a * Math.cos(angle - 0.42), y - a * Math.sin(angle - 0.42));
    ctx.lineTo(x - a * 0.7 * Math.cos(angle), y - a * 0.7 * Math.sin(angle));
    ctx.lineTo(x - a * Math.cos(angle + 0.42), y - a * Math.sin(angle + 0.42));
    ctx.fill();
  }

  const label = (n: Node) => (hub(n) ? (n.title.length > 30 ? `${n.title.slice(0, 29)}…` : n.title) : `${n.authors.split(/[ ,]/)[0]} ${n.year ?? ''}`.trim());
  const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');

  // Mouse: what is under it, in the network's coordinates.
  const world = (e: MouseEvent) => {
    const r = canvas.getBoundingClientRect();
    return { x: (e.clientX - r.left - view.x) / view.k, y: (e.clientY - r.top - view.y) / view.k, sx: e.clientX - r.left, sy: e.clientY - r.top };
  };

  function hit(x: number, y: number): Node | Edge | null {
    let best: Node | null = null;
    let bestD = Infinity;
    for (const n of nodes) {
      const d = Math.hypot(n.x! - x, n.y! - y);
      if (d < radius(n) + 4 / view.k && d < bestD) [best, bestD] = [n, d];
    }
    if (best) return best;
    const tolerance = 5 / view.k;
    // Topmost first: citations are drawn last.
    for (const e of [...edges].reverse()) {
      const [ax, ay, cx, cy, bx, by] = curve(e);
      // The curve as 12 straight pieces, near enough for the mouse.
      for (let i = 0; i < 12; i++) {
        const at = (t: number) => [(1 - t) ** 2 * ax + 2 * (1 - t) * t * cx + t ** 2 * bx, (1 - t) ** 2 * ay + 2 * (1 - t) * t * cy + t ** 2 * by];
        const [[px, py], [qx, qy]] = [at(i / 12), at((i + 1) / 12)];
        const len = (qx - px) ** 2 + (qy - py) ** 2 || 1;
        const t = Math.max(0, Math.min(1, ((x - px) * (qx - px) + (y - py) * (qy - py)) / len));
        if (Math.hypot(px + t * (qx - px) - x, py + t * (qy - py) - y) < tolerance) return e;
      }
    }
    return null;
  }

  // Why no link shown reaches a paper.
  function why(n: Node): string {
    if (!n.refs) return n.ids ? "Its references aren't looked up yet: the server does that in the background, or `carrel refs " + n.id + '`.' : "No DOI or arXiv id, so its references can't be looked up: add one to the entry.";
    const shared = (data?.missing ?? []).filter((m) => m.cited_by?.includes(n.id)).length;
    const cites = (data?.cites ?? []).some(([a, b]) => a === n.id || b === n.id);
    if (cites) return 'Its citations are hidden: show Citations.';
    return `It cites none of the papers shown and none cite it.${shared ? ` It shares ${shared} reference${shared > 1 ? 's' : ''} the library lacks with them: show Missing.` : ''}`;
  }

  function describe(h: Node | Edge): string {
    if ('degree' in h && h.ghost) {
      return `<b>${esc(h.title)}</b><br><span>${esc([h.authors, h.venue, h.year].filter(Boolean).join(' · '))}</span><br><span>Not in your library; cited by ${h.ghost.cited_by?.length} of the papers shown</span>`;
    }
    if ('degree' in h && hub(h)) {
      return `<b>${esc(h.title)}</b><br><span>${esc([h.place?.country, `${h.degree} papers`].filter(Boolean).join(' · '))}</span>`;
    }
    if ('degree' in h) {
      const p = app.papers[h.id];
      const counts = [h.cited_by != null && `cited ${h.cited_by.toLocaleString()} times`, h.h_index != null && `authors' h-index up to ${h.h_index}`].filter(Boolean).join(' · ');
      const lonely = h.degree === 0 ? `<br><span>${esc(why(h))}</span>` : '';
      return `<b>${esc(h.title)}</b><br><span>${esc([h.authors, h.venue, h.year].filter(Boolean).join(' · '))}</span>${counts ? `<br><span>${esc(counts)}</span>` : ''}${p?.keywords.length ? `<br><span class="k">${esc(p.keywords.slice(0, 5).join(', '))}</span>` : ''}${lonely}`;
    }
    const [a, b] = [label(h.source), label(h.target)];
    if (h.kind === 'cites') {
      const self = h.self ? `<br><span>A self-citation: both are by ${h.self.map(esc).join(', ')}</span>` : '';
      return `<b>${esc(a)}</b> cites <b>${esc(b)}</b><br><span>${esc(h.target.title)}</span>${self}`;
    }
    if (h.kind === 'missing') return `<b>${esc(a)}</b> cites <b>${esc(b)}</b>, not in your library<br><span>${esc(h.target.title)}</span>`;
    if (h.kind === 'institutions') return `<b>${esc(a)}</b> from <b>${esc(h.target.title)}</b>:<br>${h.names!.map(esc).join('<br>')}`;
    if (h.kind === 'similar') return `<b>${esc(a)}</b> and <b>${esc(b)}</b> are alike: ${h.score!.toFixed(2)}`;
    return `<b>${esc(a)}</b> by <b>${esc(h.target.title)}</b><br><span>${esc(h.source.title)}</span>`;
  }

  let drag: { node?: Node; x: number; y: number; moved: boolean } | null = null;

  function down(e: MouseEvent) {
    const w = world(e);
    const h = hit(w.x, w.y);
    drag = { node: h && 'degree' in h ? h : undefined, x: e.clientX, y: e.clientY, moved: false };
    if (drag.node) {
      drag.node.fx = drag.node.x;
      drag.node.fy = drag.node.y;
      sim?.alphaTarget(0.25).restart();
    }
  }

  function move(e: MouseEvent) {
    const w = world(e);
    if (drag) {
      const [dx, dy] = [e.clientX - drag.x, e.clientY - drag.y];
      if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
      if (drag.node) [drag.node.fx, drag.node.fy] = [w.x, w.y];
      else if (drag.moved) {
        view.x += e.clientX - drag.x;
        view.y += e.clientY - drag.y;
        [drag.x, drag.y] = [e.clientX, e.clientY];
      }
      tip = null;
      draw();
      return;
    }
    hovered = hit(w.x, w.y);
    canvas.style.cursor = hovered ? 'pointer' : 'grab';
    tip = hovered ? { x: w.sx, y: w.sy, html: describe(hovered) } : null;
    draw();
  }

  function up(e: MouseEvent) {
    if (drag?.node) {
      [drag.node.fx, drag.node.fy] = [null, null];
      sim?.alphaTarget(0);
    }
    if (drag && !drag.moved) {
      const w = world(e);
      const h = hit(w.x, w.y);
      selected = h && 'degree' in h ? (selected === h ? null : h) : null;
      draw();
    }
    drag = null;
  }

  function wheel(e: WheelEvent) {
    e.preventDefault();
    const w = world(e);
    const k = Math.min(6, Math.max(0.2, view.k * Math.exp(-e.deltaY * 0.0015)));
    view = { k, x: w.sx - w.x * k, y: w.sy - w.y * k };
    tip = null;
    draw();
  }

  $effect(() => {
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      const first = size.w === 800 && size.h === 600;
      size = { w: width, h: height };
      if (first) view = { x: width / 2, y: height / 2, k: 0.9 };
      const dpr = devicePixelRatio || 1;
      canvas.width = width * dpr;
      canvas.height = height * dpr;
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      draw();
    });
    observer.observe(box);
    return () => observer.disconnect();
  });

  // Redraw when the colours or the theme change.
  $effect(() => {
    void [colorBy, groups, range];
    draw();
  });
  const themes = new MutationObserver(draw);
  $effect(() => {
    themes.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => themes.disconnect();
  });

  // The papers alone, counted by why, for the "Linked only" tooltip.
  const reasons = $derived.by(() => {
    const n = (f: (p: Node) => boolean) => alone.filter(f).length;
    const lines = [
      [n((p) => !p.refs && !!p.ids), "references not looked up yet"],
      [n((p) => !p.ids && !p.refs), 'no DOI or arXiv id to look references up by'],
      [n((p) => !!p.refs), 'cite none of the others, none cite them'],
    ] as const;
    const text = lines.filter(([c]) => c).map(([c, why]) => `${c}: ${why}`);
    return text.length ? `Papers no link shown reaches:\n${text.join('\n')}\nHover one with this off to see why` : 'Hide papers no link shown reaches';
  });

  const counts = $derived(
    data ? { cites: data.cites.length, authors: new Set(data.authors.flatMap(([, , names]) => names)).size, institutions: data.institutions.length, similar: data.similar.filter(([, , s]) => s >= alike).length } : null,
  );
</script>

<div class="flex h-full flex-col">
  <div class="flex items-center gap-x-4 overflow-x-auto border-b px-4 py-2.5 text-sm whitespace-nowrap [scrollbar-width:none]">
    <select bind:value={project} class="h-8 w-44 shrink-0 cursor-pointer rounded-lg border bg-card px-2 text-sm outline-none">
      <option value="">Whole library</option>
      {#each app.projects as p}<option value={p.name}>{p.name}</option>{/each}
    </select>
    <label class="flex cursor-pointer items-center gap-1.5"><input type="checkbox" bind:checked={show.cites} class="accent-[var(--accent)]" /><span class="h-0.5 w-4 rounded bg-accent"></span>Citations {#if counts}<span class="text-xs text-faint">{counts.cites}</span>{/if}</label>
    <label class="flex cursor-pointer items-center gap-1.5" title="Papers the library lacks that several of those shown cite, as grey rings: they may tie together papers nothing else links">
      <input type="checkbox" bind:checked={show.missing} class="accent-[var(--accent)]" /><span class="size-2.5 rounded-full border-2 border-faint"></span>Missing
      {#if show.missing}<input type="range" min="2" max="10" step="1" bind:value={often} class="w-16 accent-[var(--accent)]" /><span class="w-12 text-xs text-faint tabular-nums">cited ≥{often}×</span>{/if}
    </label>
    <label class="flex cursor-pointer items-center gap-1.5" title="Authors two or more papers share, as diamonds"><input type="checkbox" bind:checked={show.authors} class="accent-[var(--accent)]" /><span class="size-2 rotate-45 border-2 border-warning"></span>Authors {#if counts}<span class="text-xs text-faint">{counts.authors}</span>{/if}</label>
    <label class="flex cursor-pointer items-center gap-1.5" title={counts?.institutions === 0 ? 'None looked up yet: `carrel refs --all` asks OpenAlex where the authors worked' : 'Institutions two or more papers come from, as squares'}>
      <input type="checkbox" bind:checked={show.institutions} class="accent-[var(--accent)]" /><span class="size-2.5 rounded-sm border-2 border-title"></span>Institutions {#if counts}<span class="text-xs text-faint">{counts.institutions}</span>{/if}
    </label>
    <label class="flex cursor-pointer items-center gap-1.5" title="From the embeddings: papers alike in title, keywords and abstract">
      <input type="checkbox" bind:checked={show.similar} class="accent-[var(--accent)]" /><span class="h-0.5 w-4 border-t-2 border-dashed border-ready"></span>Alike
      {#if show.similar}<input type="range" min="0.5" max="0.95" step="0.01" bind:value={alike} class="w-20 accent-[var(--ready)]" /><span class="w-8 text-xs text-faint tabular-nums">≥{alike.toFixed(2)}</span>{/if}
    </label>
    <label class="flex cursor-pointer items-center gap-1.5" title={reasons}><input type="checkbox" bind:checked={hideLonely} class="accent-[var(--accent)]" />Linked only {#if alone.length}<span class="text-xs text-faint">{alone.length} alone</span>{/if}</label>
    <label class="ml-auto flex shrink-0 items-center gap-2 text-muted">
      Colour by
      <select bind:value={colorBy} class="h-8 cursor-pointer rounded-lg border bg-card px-2 text-sm text-foreground outline-none">
        {#each MENU as [label, values]}<optgroup {label}>{#each values as value}<option {value}>{COLORS[value]}</option>{/each}</optgroup>{/each}
      </select>
    </label>
  </div>

  <div bind:this={box} class="relative min-h-0 flex-1 overflow-hidden">
    <canvas
      bind:this={canvas}
      class="absolute inset-0 cursor-grab"
      onmousedown={down}
      onmousemove={move}
      onmouseup={up}
      onmouseleave={() => ((tip = null), (hovered = null), draw())}
      ondblclick={(e) => {
        const w = world(e);
        const h = hit(w.x, w.y);
        if (h && 'degree' in h && !hub(h) && !h.ghost) go(`/paper/${encodeURIComponent(h.id)}`);
      }}
      onwheel={wheel}
    ></canvas>

    {#if !data && !error}
      <div class="absolute inset-0 flex items-center justify-center"><Spinner class="size-6 text-faint" /></div>
    {/if}
    {#if error}<div class="absolute top-4 left-4 rounded-lg border border-failed/40 bg-failed/10 px-3 py-2 text-sm text-failed">{error}</div>{/if}

    {#if tip}
      <div
        class="pointer-events-none absolute z-10 max-w-sm rounded-lg border bg-card px-3 py-2 text-xs leading-relaxed shadow-lg [&_.k]:text-faint [&_span]:text-muted"
        style="left: {Math.min(tip.x + 14, size.w - 320)}px; top: {tip.y + 14}px"
      >
        {@html tip.html}
      </div>
    {/if}

    {#if selected}
      {@const s = selected}
      <div class="absolute bottom-4 left-4 z-10 w-96 max-w-[calc(100%-2rem)] rounded-xl border bg-card p-4 shadow-xl">
        <div class="flex items-start gap-2">
          <div class="min-w-0 flex-1">
            <div class="text-sm leading-snug font-medium">{s.title}</div>
            <div class="mt-1 text-xs text-muted">{[s.authors, s.venue, s.year].filter(Boolean).join(' · ')}</div>
          </div>
          <button class="cursor-pointer text-faint hover:text-foreground" onclick={() => ((selected = null), draw())} aria-label="Close"><X class="size-4" /></button>
        </div>
        <div class="mt-3 flex items-center gap-3 text-xs text-muted">
          <span class="shrink-0">{s.degree} {hub(s) || s.ghost ? 'papers' : 'links'}</span>
          {#if s.ghost}<span>not in your library</span>{/if}
          {#if s.projects.length}<span class="min-w-0 truncate">in {s.projects.join(', ')}</span>{/if}
          {#if s.place?.ror}
            <a href={s.place.ror} target="_blank" rel="noreferrer" class="ml-auto flex shrink-0 items-center gap-1 font-medium text-accent hover:underline">ROR<ArrowUpRight class="size-3.5" /></a>
          {:else if s.person}
            <a href="#/people?name={encodeURIComponent(s.person)}" class="ml-auto flex shrink-0 items-center gap-1 font-medium text-accent hover:underline">Their papers<ArrowUpRight class="size-3.5" /></a>
          {:else if s.ghost}
            {@const g = s.ghost}
            <button
              class="ml-auto flex shrink-0 cursor-pointer items-center gap-1 font-medium text-accent hover:underline disabled:opacity-50"
              disabled={adding}
              onclick={async () => ((adding = true), (await addSource(g.doi ?? (g.arxiv ? `arXiv:${g.arxiv}` : g.title), false)) && loads++, (adding = false))}
            >{adding ? 'Adding…' : 'Add to library'}<Plus class="size-3.5" /></button>
          {:else if !s.place}
            <a href={paperHref(s.id)} class="ml-auto flex shrink-0 items-center gap-1 font-medium text-accent hover:underline">Open<ArrowUpRight class="size-3.5" /></a>
          {/if}
        </div>
        {#if !hub(s) && !s.ghost && s.degree === 0}<div class="mt-2 text-xs text-muted">{why(s)}</div>{/if}
      </div>
    {/if}

    {#if colorBy === 'group'}
      <div class="pointer-events-none absolute top-3 right-3 max-h-[60%] space-y-1 overflow-hidden rounded-lg border bg-card/90 px-3 py-2 text-xs" title="Papers the links shown tie together">
        {#each groups as g, i}<div class="flex items-center gap-2"><span class="size-2.5 rounded-full" style="background: {palette[i]}"></span>{g.name}<span class="text-faint">{g.size}</span></div>{/each}
        <div class="flex items-center gap-2 text-faint"><span class="size-2.5 rounded-full bg-faint"></span>{groups.length ? 'smaller groups, alone' : 'no groups: show some links'}</div>
      </div>
    {:else if colorBy === 'type'}
      <div class="pointer-events-none absolute top-3 right-3 space-y-1 rounded-lg border bg-card/90 px-3 py-2 text-xs">
        {#each Object.values(TYPES) as [name, c]}<div class="flex items-center gap-2"><span class="size-2.5 rounded-full" style="background: {c}"></span>{name}</div>{/each}
        <div class="flex items-center gap-2 text-faint"><span class="size-2.5 rounded-full bg-faint"></span>other</div>
      </div>
    {:else if colorBy === 'read'}
      <div class="pointer-events-none absolute top-3 right-3 space-y-1 rounded-lg border bg-card/90 px-3 py-2 text-xs">
        {#each Object.values(READ) as [name, c]}<div class="flex items-center gap-2"><span class="size-2.5 rounded-full" style="background: {c}"></span>{name}</div>{/each}
        <div class="flex items-center gap-2 text-faint"><span class="size-2.5 rounded-full bg-faint"></span>nothing yet</div>
      </div>
    {:else}
      {@const [low, high, how] = ENDS[colorBy] ?? ['', '']}
      <div class="pointer-events-none absolute top-3 right-3 w-52 rounded-lg border bg-card/90 px-3 py-2 text-xs">
        <div class="h-2 rounded" style="background: linear-gradient(to right, hsl(220 70% 58%), hsl(120 70% 58%), hsl(20 70% 58%))"></div>
        <div class="mt-1 flex justify-between text-faint">
          <span>{colorBy === 'year' ? `≤ ${range.lo}` : `${low} ${shown(range.lo)}`}</span>
          <span>{colorBy === 'year' ? `≥ ${range.hi}` : `${shown(range.hi)} ${high}`}</span>
        </div>
        {#if how}<div class="mt-1 text-faint">{how}</div>{/if}
        {#if colorBy === 'hindex' && !show.authors}
          <div class="mt-1 text-faint">Show Authors to see them</div>
        {:else if (colorBy === 'citations' || colorBy === 'momentum') && range.known < (data?.nodes.length ?? 0)}
          <div class="mt-1 flex items-center gap-2 text-faint"><span class="size-2.5 shrink-0 rounded-full bg-faint"></span>not looked up yet: the server does, or <code>carrel refs --all</code></div>
        {/if}
      </div>
    {/if}

    <div class="pointer-events-none absolute right-3 bottom-3 text-xs text-faint">Scroll to zoom · drag to move · click a paper to see its links · double-click opens it</div>
  </div>
</div>
