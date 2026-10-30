// Routes live in the hash (#/paper/vaswani2017attention), so the server only ever serves index.html.

class Router {
  path = $state('/');
  params = $state(new URLSearchParams());

  constructor() {
    const read = () => {
      const [path, query = ''] = (location.hash.slice(1) || '/').split('?');
      this.path = path;
      this.params = new URLSearchParams(query);
    };
    addEventListener('hashchange', read);
    read();
  }

  /** The route's segments: `/paper/x` gives `['paper', 'x']`. */
  get parts() {
    return this.path.split('/').filter(Boolean).map(decodeURIComponent);
  }
}

export const router = new Router();

export function href(path: string, params: Record<string, string | undefined> = {}) {
  const search = new URLSearchParams(Object.entries(params).filter(([, v]) => v) as [string, string][]).toString();
  return `#${path}${search ? `?${search}` : ''}`;
}

export function go(path: string, params: Record<string, string | undefined> = {}) {
  location.hash = href(path, params).slice(1);
}

export const paperHref = (id: string) => href(`/paper/${encodeURIComponent(id)}`);
