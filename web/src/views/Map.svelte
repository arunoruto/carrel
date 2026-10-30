<script lang="ts">
  import { ArrowUpRight, X } from '@lucide/svelte';
  import {
    forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY,
    type Simulation, type SimulationNodeDatum,
  } from 'd3-force';
  import { untrack } from 'svelte';
  import { api, type LibraryMap, type MapNode } from '../lib/api';
  import Spinner from '../lib/components/Spinner.svelte';
  import { go, paperHref, router } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';

  // The library as a network: papers linked by citations, by the authors they share (thicker for more), through
  // the institutions they come from (square hubs), and by how alike they are. Drawn on a canvas; d3-force lays it out.
  type Place = LibraryMap['institutions'][number];
  type Node = MapNode & SimulationNodeDatum & { degree: number; place?: Place };
  type Kind = 'cites' | 'authors' | 'institutions' | 'similar';
  // `bend`: how far a link curves off the straight line, so two papers linked several ways show each link.
  // `self`: for a citation, the authors the two papers share.
  type Edge = { source: Node; target: Node; kind: Kind; names?: string[]; score?: number; bend: number; self?: string[] };

  // Raw: d3 moves the nodes itself, and the selection must stay the very node object.
  let data = $state.raw<LibraryMap | null>(null);
  let error = $state('');
  let project = $state(router.params.get('project') ?? '');
  // Which links to draw, remembered in this browser; citations alone to begin with, all of them at once is a lot.
  const LINKS = 'carrel-map-links';
  let show = $state<Record<Kind, boolean>>({ cites: true, authors: false, institutions: false, similar: false, ...stored() });
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
  let colorBy = $state<'project' | 'year'>('project');
  let hideLonely = $state(true);
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

  $effect(() => {
    const p = project;
    data = null;
    api.map(p || undefined).then((m) => ((data = m), (error = ''))).catch((e) => (error = e.message));
  });

  // (Re)build the network when the data or the choices change, keeping where papers already are.
  $effect(() => {
    if (!data) return;
    const d = data;
    const [s, threshold, lonely] = [{ ...show }, alike, hideLonely];
    const before = new Map(byId);
    const place = (p: Place): MapNode & { place: Place } => {
      const id = `institution:${p.ror ?? p.name}`;
      return { id, title: p.name, authors: '', year: null, venue: p.country, keywords: [], projects: [], place: p };
    };
    const all = [...d.nodes, ...(s.institutions ? d.institutions.map(place) : [])].map((n) => {
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
    if (s.authors) d.authors.forEach(([a, b, names]) => add(a, b, 'authors', { names }));
    if (s.institutions) d.institutions.forEach((p) => Object.entries(p.papers).forEach(([id, names]) => add(id, place(p).id, 'institutions', { names })));
    if (s.similar) d.similar.filter(([, , sc]) => sc >= threshold).forEach(([a, b, score]) => add(a, b, 'similar', { score }));
    // Papers linked more than one way get their links side by side, so a citation between co-authors stays visible.
    const pairs = new Map<string, Edge[]>();
    list.forEach((e) => pairs.set(pair(e.source.id, e.target.id), [...(pairs.get(pair(e.source.id, e.target.id)) ?? []), e]));
    pairs.forEach((group) => group.forEach((e, i) => (e.bend = (i - (group.length - 1) / 2) * 22)));
    // Citations last, so they are drawn on top.
    const order: Record<Kind, number> = { similar: 0, institutions: 1, authors: 2, cites: 3 };
    list.sort((a, b) => order[a.kind] - order[b.kind]);
    list.forEach((e) => (e.source.degree++, e.target.degree++));
    nodes = lonely ? all.filter((n) => n.degree > 0) : all;
    edges = list;
    untrack(() => selected && !nodes.includes(selected) && (selected = null));
    const warm = !!sim;
    sim?.stop();
    sim = forceSimulation(nodes)
      .force(
        'link',
        forceLink<Node, Edge>(edges)
          .distance((e) => ({ authors: 46, cites: 70, institutions: 60, similar: 80 })[e.kind])
          .strength((e) => (e.kind === 'authors' ? Math.min(0.9, 0.25 * (e.names?.length ?? 1)) : e.kind === 'cites' ? 0.25 : e.kind === 'institutions' ? 0.2 : 0.15)),
      )
      .force('charge', forceManyBody().strength(-170).distanceMax(600))
      .force('x', forceX(0).strength(0.05))
      .force('y', forceY(0).strength(0.05))
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

  const radius = (n: Node) => (n.place ? 5 + Math.sqrt(n.degree) * 2 : 4 + Math.sqrt(n.degree) * 1.8);
  const palette = ['#e8586e', '#f2a33a', '#4fb06d', '#4c7ef0', '#a974e8', '#2fb5c4', '#e077b8', '#b5b83a', '#d9773a', '#7c8bd9', '#3fa38f', '#c46b4f'];
  const projects = $derived(app.projects.map((p) => p.name));
  const sizes = $derived(Object.fromEntries(app.projects.map((p) => [p.name, p.papers.length])));
  const years = $derived.by(() => {
    // The middle 90%, so one classic from 1865 doesn't turn everything else the same colour.
    const ys = (data?.nodes ?? []).map((n) => n.year).filter((y): y is number => !!y).sort((a, b) => a - b);
    const at = (q: number) => ys[Math.round(q * (ys.length - 1))];
    return ys.length ? [at(0.05), at(0.95)] : [2000, 2025];
  });

  function color(n: Node): string {
    if (colorBy === 'year') {
      if (!n.year) return css('--faint');
      const t = Math.min(1, Math.max(0, (n.year - years[0]) / Math.max(1, years[1] - years[0])));
      return `hsl(${220 - t * 200} 70% 58%)`;
    }
    // The paper's smallest project says most about it; big catch-all projects would colour everything alike.
    const own = [...n.projects].sort((a, b) => (sizes[a] ?? 0) - (sizes[b] ?? 0))[0];
    return own ? palette[projects.indexOf(own) % palette.length] : css('--faint');
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
    const kinds: Record<Kind, string> = { cites: css('--accent'), authors: css('--warning'), institutions: css('--title'), similar: css('--ready') };
    for (const e of edges) {
      const on = !focus || e.source === focus || e.target === focus;
      ctx.globalAlpha = e === hovered ? 1 : on ? (focus ? 0.9 : e.kind === 'similar' ? 0.7 : e.kind === 'cites' ? 0.5 : 0.35) : 0.06;
      ctx.strokeStyle = kinds[e.kind];
      const width = e.kind === 'authors' ? 0.8 + 1.4 * Math.min(6, e.names?.length ?? 1) : e.kind === 'similar' ? 1 + 6 * Math.max(0, (e.score ?? 0) - 0.6) : e.kind === 'cites' ? 1.5 : 1.2;
      ctx.lineWidth = (width / Math.sqrt(view.k)) * (e === hovered ? 2 : 1);
      ctx.setLineDash(e.kind === 'similar' ? [4, 4] : []);
      const [ax, ay, cx, cy, bx, by] = curve(e);
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.quadraticCurveTo(cx, cy, bx, by);
      ctx.stroke();
      if (e.kind === 'cites' && (on || !focus)) {
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
      else ctx.arc(n.x!, n.y!, r, 0, Math.PI * 2);
      ctx.fillStyle = n.place ? css('--card') : color(n);
      ctx.fill();
      if (n.place) {
        ctx.lineWidth = 2 / view.k;
        ctx.strokeStyle = kinds.institutions;
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
      if (view.k > 1.8 || (near && lit(n)) || (n.place && n.degree >= 5 && view.k > 0.7)) {
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

  const label = (n: Node) => (n.place ? (n.title.length > 30 ? `${n.title.slice(0, 29)}…` : n.title) : `${n.authors.split(/[ ,]/)[0]} ${n.year ?? ''}`.trim());
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

  function describe(h: Node | Edge): string {
    if ('degree' in h && h.place) {
      return `<b>${esc(h.title)}</b><br><span>${esc([h.place.country, `${h.degree} papers`].filter(Boolean).join(' · '))}</span>`;
    }
    if ('degree' in h) {
      const p = app.papers[h.id];
      return `<b>${esc(h.title)}</b><br><span>${esc([h.authors, h.venue, h.year].filter(Boolean).join(' · '))}</span>${p?.keywords.length ? `<br><span class="k">${esc(p.keywords.slice(0, 5).join(', '))}</span>` : ''}`;
    }
    const [a, b] = [label(h.source), label(h.target)];
    if (h.kind === 'cites') {
      const self = h.self ? `<br><span>A self-citation: both are by ${h.self.map(esc).join(', ')}</span>` : '';
      return `<b>${esc(a)}</b> cites <b>${esc(b)}</b><br><span>${esc(h.target.title)}</span>${self}`;
    }
    if (h.kind === 'institutions') return `<b>${esc(a)}</b> from <b>${esc(h.target.title)}</b>:<br>${h.names!.map(esc).join('<br>')}`;
    if (h.kind === 'similar') return `<b>${esc(a)}</b> and <b>${esc(b)}</b> are alike: ${h.score!.toFixed(2)}`;
    return `<b>${esc(a)}</b> and <b>${esc(b)}</b> share ${h.names!.length === 1 ? 'an author' : `${h.names!.length} authors`}:<br>${h.names!.map(esc).join('<br>')}`;
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
    void [colorBy, projects];
    draw();
  });
  const themes = new MutationObserver(draw);
  $effect(() => {
    themes.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => themes.disconnect();
  });

  const counts = $derived(
    data ? { cites: data.cites.length, authors: data.authors.length, institutions: data.institutions.length, similar: data.similar.filter(([, , s]) => s >= alike).length } : null,
  );
</script>

<div class="flex h-full flex-col">
  <div class="flex items-center gap-x-4 overflow-x-auto border-b px-4 py-2.5 text-sm whitespace-nowrap [scrollbar-width:none]">
    <select bind:value={project} class="h-8 w-44 shrink-0 cursor-pointer rounded-lg border bg-card px-2 text-sm outline-none">
      <option value="">Whole library</option>
      {#each app.projects as p}<option value={p.name}>{p.name}</option>{/each}
    </select>
    <label class="flex cursor-pointer items-center gap-1.5"><input type="checkbox" bind:checked={show.cites} class="accent-[var(--accent)]" /><span class="h-0.5 w-4 rounded bg-accent"></span>Citations {#if counts}<span class="text-xs text-faint">{counts.cites}</span>{/if}</label>
    <label class="flex cursor-pointer items-center gap-1.5"><input type="checkbox" bind:checked={show.authors} class="accent-[var(--accent)]" /><span class="h-1 w-4 rounded bg-warning"></span>Authors {#if counts}<span class="text-xs text-faint">{counts.authors}</span>{/if}</label>
    <label class="flex cursor-pointer items-center gap-1.5" title={counts?.institutions === 0 ? 'None looked up yet: `carrel refs --all` asks OpenAlex where the authors worked' : 'Institutions two or more papers come from, as squares'}>
      <input type="checkbox" bind:checked={show.institutions} class="accent-[var(--accent)]" /><span class="size-2.5 rounded-sm border-2 border-title"></span>Institutions {#if counts}<span class="text-xs text-faint">{counts.institutions}</span>{/if}
    </label>
    <label class="flex cursor-pointer items-center gap-1.5" title="From the embeddings: papers alike in title, keywords and abstract">
      <input type="checkbox" bind:checked={show.similar} class="accent-[var(--accent)]" /><span class="h-0.5 w-4 border-t-2 border-dashed border-ready"></span>Alike
      {#if show.similar}<input type="range" min="0.5" max="0.95" step="0.01" bind:value={alike} class="w-20 accent-[var(--ready)]" /><span class="w-8 text-xs text-faint tabular-nums">≥{alike.toFixed(2)}</span>{/if}
    </label>
    <label class="flex cursor-pointer items-center gap-1.5"><input type="checkbox" bind:checked={hideLonely} class="accent-[var(--accent)]" />Linked only</label>
    <div class="ml-auto flex rounded-lg border bg-card p-0.5 text-xs">
      <button class="cursor-pointer rounded-md px-2.5 py-1 {colorBy === 'project' ? 'bg-hover font-medium' : 'text-muted'}" onclick={() => (colorBy = 'project')}>Project</button>
      <button class="cursor-pointer rounded-md px-2.5 py-1 {colorBy === 'year' ? 'bg-hover font-medium' : 'text-muted'}" onclick={() => (colorBy = 'year')}>Year</button>
    </div>
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
        if (h && 'degree' in h && !h.place) go(`/paper/${encodeURIComponent(h.id)}`);
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
          <span class="shrink-0">{s.degree} {s.place ? 'papers' : 'links'}</span>
          {#if s.projects.length}<span class="min-w-0 truncate">in {s.projects.join(', ')}</span>{/if}
          {#if s.place?.ror}
            <a href={s.place.ror} target="_blank" rel="noreferrer" class="ml-auto flex shrink-0 items-center gap-1 font-medium text-accent hover:underline">ROR<ArrowUpRight class="size-3.5" /></a>
          {:else if !s.place}
            <a href={paperHref(s.id)} class="ml-auto flex shrink-0 items-center gap-1 font-medium text-accent hover:underline">Open<ArrowUpRight class="size-3.5" /></a>
          {/if}
        </div>
      </div>
    {/if}

    {#if colorBy === 'project' && projects.length}
      <div class="pointer-events-none absolute top-3 right-3 max-h-[60%] space-y-1 overflow-hidden rounded-lg border bg-card/90 px-3 py-2 text-xs">
        {#each projects as name, i}<div class="flex items-center gap-2"><span class="size-2.5 rounded-full" style="background: {palette[i % palette.length]}"></span>{name}</div>{/each}
        <div class="flex items-center gap-2 text-faint"><span class="size-2.5 rounded-full bg-faint"></span>no project</div>
      </div>
    {:else if colorBy === 'year'}
      <div class="pointer-events-none absolute top-3 right-3 rounded-lg border bg-card/90 px-3 py-2 text-xs">
        <div class="h-2 w-40 rounded" style="background: linear-gradient(to right, hsl(220 70% 58%), hsl(120 70% 58%), hsl(20 70% 58%))"></div>
        <div class="mt-1 flex justify-between text-faint"><span>≤ {years[0]}</span><span>≥ {years[1]}</span></div>
      </div>
    {/if}

    <div class="pointer-events-none absolute right-3 bottom-3 text-xs text-faint">Scroll to zoom · drag to move · click a paper to see its links · double-click opens it</div>
  </div>
</div>
