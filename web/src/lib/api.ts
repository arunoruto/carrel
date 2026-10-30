// The JSON API `carrel web` serves under /api (src/web.rs).

export type FieldName = 'summary' | 'keywords' | 'notes' | 'references';

export interface PaperSummary {
  id: string;
  title: string;
  authors: string[];
  year: number | null;
  venue: string | null;
  keywords: string[];
  pdf: boolean;
  text: boolean;
  drafts: FieldName[];
  added: number | null;
  snippet?: string;
}

export interface Paper {
  id: string;
  title: string;
  authors: string[];
  year: number | null;
  venue: string | null;
  doi: string | null;
  arxiv: string | null;
  url: string | null;
  abstract: string | null;
  keywords: string[];
  summary: string | null;
  notes: string | null;
  figures: string[];
  supplementary: string[];
  fulltext_chars: number | null;
  projects: string[];
  drafts: Partial<Record<FieldName, string>>;
  bibtex: string;
  pdf: boolean;
  text: boolean;
}

export interface Reference {
  title: string;
  authors?: string[];
  year?: number | null;
  venue?: string;
  doi?: string | null;
  arxiv?: string | null;
  source?: string;
  in_library: string | null;
  add_with?: string | null;
  cited_by?: string[];
}

export interface Info {
  version: string;
  library: string;
  remote: boolean;
  embeddings: { model: string; url: string } | { error: string } | null;
  drafts: number;
  ui: boolean;
}

export interface Draft {
  id: string;
  title: string;
  field: FieldName;
  draft: string | null;
  current: string | null;
}

export interface Project {
  name: string;
  papers: string[];
}

export interface InboxFile {
  path: string;
  name: string;
  size: number;
  modified: number;
  clue: string | null;
  skipped: boolean;
}

export interface Inbox {
  folders: string[];
  days: number;
  files: InboxFile[];
}

export interface Person {
  id?: string;
  name: string;
  variants: string[];
  papers: string[];
  coauthors: [string, number][];
  maybe: { name: string; papers: string[]; spellings: string[]; with: string[] }[];
}

export interface PeopleDraft {
  id: string;
  same: boolean;
  spellings: string[];
  reason: string;
}

export interface TreeNode {
  level: number;
  id: string;
  again: boolean;
}

export interface MapNode {
  id: string;
  title: string;
  authors: string;
  year: number | null;
  venue: string | null;
  keywords: string[];
  projects: string[];
}

export interface LibraryMap {
  nodes: MapNode[];
  cites: [string, string][];
  authors: [string, string, string[]][];
  similar: [string, string, number][];
}

export interface Issue {
  level: 'error' | 'warning' | 'info';
  check: string;
  id: string;
  message: string;
  fix: string | null;
}

export interface Neighbors {
  cites: string[];
  cited_by: string[];
  shared_authors: Record<string, string[]>;
  references_known: boolean;
}

export interface Added {
  id: string;
  messages: string[];
}

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: BodyInit, json = true): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined && typeof body === 'string') headers['Content-Type'] = 'application/json';
  const response = await fetch(`/api/${path}`, { method, body, headers, credentials: 'same-origin' });
  if (!response.ok) {
    let message = response.statusText;
    try {
      message = (await response.json()).error ?? message;
    } catch {}
    throw new ApiError(message, response.status);
  }
  return (json ? response.json() : response.text()) as Promise<T>;
}

const q = (params: Record<string, string | number | boolean | undefined>) => {
  const search = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== '' && v !== false) search.set(k, String(v));
  const s = search.toString();
  return s ? `?${s}` : '';
};

const paper = (id: string) => `papers/${encodeURIComponent(id)}`;

export const api = {
  info: () => call<Info>('GET', 'info'),
  papers: (project?: string) => call<PaperSummary[]>('GET', `papers${q({ project })}`),
  search: (query: string, semantic: boolean, limit = 100) =>
    call<PaperSummary[]>('GET', `search${q({ q: query, semantic: semantic ? 1 : undefined, limit })}`),
  paper: (id: string) => call<Paper>('GET', paper(id)),
  fulltext: (id: string) => call<string>('GET', `${paper(id)}/fulltext`, undefined, false),
  add: (source: string) => call<Added>('POST', 'add', JSON.stringify({ source })),
  upload: (file: File) => call<Added>('POST', `upload${q({ name: file.name })}`, file),
  remove: (id: string) => call<{}>('DELETE', paper(id)),
  attach: (id: string, file?: File) => call<{ messages: string[] }>('POST', `${paper(id)}/pdf`, file ?? ''),
  supplementary: (id: string, file: File) =>
    call<{ messages: string[] }>('POST', `${paper(id)}/supplementary${q({ name: file.name })}`, file),
  notes: (id: string, notes: string) => call<Paper>('PUT', `${paper(id)}/notes`, new Blob([notes])),
  keywords: (id: string, keywords: string[]) => call<Paper>('PUT', `${paper(id)}/keywords`, JSON.stringify({ keywords })),
  update: (id: string) => call<{ messages: string[] }>('POST', `${paper(id)}/update`),
  extract: (id: string) => call<{}>('POST', `${paper(id)}/extract`),
  references: (id: string, refresh = false) => call<Reference[]>('GET', `${paper(id)}/references${q({ refresh })}`),
  citedBy: (id: string) => call<Reference[]>('GET', `${paper(id)}/cited-by`),
  decide: (id: string, field: FieldName, approve: boolean) =>
    call<Paper>('POST', `${paper(id)}/drafts/${field}/${approve ? 'approve' : 'reject'}`),
  toProject: (id: string, project: string, add: boolean) =>
    call<{ projects: string[] }>(add ? 'PUT' : 'DELETE', `${paper(id)}/projects/${encodeURIComponent(project)}`),
  drafts: () => call<Draft[]>('GET', 'drafts'),
  projects: () => call<Project[]>('GET', 'projects'),
  suggestions: (project?: string) => call<Reference[]>('GET', `suggestions${q({ project })}`),
  inbox: () => call<Inbox>('GET', 'inbox'),
  consume: (path: string) => call<Added>('POST', 'inbox/add', JSON.stringify({ path })),
  skip: (path: string) => call<{}>('POST', 'inbox/skip', JSON.stringify({ path })),
  sync: () => call<{ summary: string; failed: string[] }>('POST', 'sync'),
  tree: (id: string, citedBy: boolean, depth: number) => call<TreeNode[]>('GET', `${paper(id)}/tree${q({ cited_by: citedBy, depth })}`),
  similar: (id: string) => call<{ id: string; score: number }[]>('GET', `${paper(id)}/similar`),
  neighbors: (id: string) => call<Neighbors>('GET', `${paper(id)}/neighbors`),
  map: (project?: string) => call<LibraryMap>('GET', `map${q({ project })}`),
  people: () => call<Person[]>('GET', 'people'),
  peopleDrafts: () => call<PeopleDraft[]>('GET', 'people/drafts'),
  decidePeople: (draft: string, approve: boolean) => call<{}>('POST', `people/drafts/${draft}/${approve ? 'approve' : 'reject'}`),
  samePerson: (spellings: string[]) => call<{ id: string }>('POST', 'people/same', JSON.stringify({ spellings })),
  differentPeople: (spellings: string[]) => call<{ ids: string[] }>('POST', 'people/different', JSON.stringify({ spellings })),
  doctor: () => call<Issue[]>('GET', 'doctor'),
  bibUrl: (project?: string) => `/api/bib${q({ project })}`,
  pdfUrl: (id: string) => `/api/${paper(id)}/pdf`,
  fileUrl: (id: string, folder: 'figures' | 'supplementary', name: string) =>
    `/api/${paper(id)}/files/${folder}/${encodeURIComponent(name)}`,
};
