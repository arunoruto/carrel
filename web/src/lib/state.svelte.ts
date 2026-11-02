import { api, ApiError, type Info, type Me, type PaperSummary, type Project } from './api';
import { toasts } from './toast.svelte';

// What the sidebar shows everywhere, refreshed after anything that changes it.
class App {
  info = $state<Info | null>(null);
  me = $state<Me | null>(null);
  projects = $state<Project[]>([]);
  inbox = $state(0);
  /** Papers recommended by alert emails, waiting in Discover. */
  recommended = $state(0);
  total = $state(0);
  /** Every paper by id, for views that only get ids (people, trees, the map). */
  papers = $state<Record<string, PaperSummary>>({});
  loggedOut = $state(false);
  /** Bumped whenever papers change, so open lists reload. */
  version = $state(0);

  async refresh() {
    try {
      this.me = await api.me();
      this.loggedOut = this.me.role === 'none';
      if (this.loggedOut) return;
      // Info asks Ollama, which may take seconds to time out; the rest doesn't wait for it.
      const info = api.info().then((info) => (this.info = info));
      const [projects, papers] = await Promise.all([api.projects(), api.papers()]);
      await info;
      this.projects = projects;
      this.total = papers.length;
      this.papers = Object.fromEntries(papers.map((p) => [p.id, p]));
      this.loggedOut = false;
      api
        .inbox()
        .then((inbox) => (this.inbox = inbox.files.filter((f) => !f.skipped).length))
        .catch(() => (this.inbox = 0));
      api
        .recommended()
        .then((r) => (this.recommended = r.length))
        .catch(() => (this.recommended = 0));
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) this.loggedOut = true;
      else toasts.error(e, 'Cannot reach carrel');
    }
  }

  changed() {
    this.version++;
    this.refresh();
  }

  /** What the browser may do; nothing until /api/me answered. */
  get can() {
    return this.me?.can ?? { read: false, add: false, edit: false, admin: false };
  }

  get semantic() {
    const e = this.info?.embeddings;
    return e && 'model' in e ? e : null;
  }
}

export const app = new App();
