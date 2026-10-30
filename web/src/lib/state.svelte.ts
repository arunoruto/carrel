import { api, ApiError, type Info, type Project } from './api';
import { toasts } from './toast.svelte';

// What the sidebar shows everywhere, refreshed after anything that changes it.
class App {
  info = $state<Info | null>(null);
  projects = $state<Project[]>([]);
  inbox = $state(0);
  total = $state(0);
  loggedOut = $state(false);
  /** Bumped whenever papers change, so open lists reload. */
  version = $state(0);

  async refresh() {
    try {
      const [info, projects, papers] = await Promise.all([api.info(), api.projects(), api.papers()]);
      this.info = info;
      this.projects = projects;
      this.total = papers.length;
      this.loggedOut = false;
      api
        .inbox()
        .then((inbox) => (this.inbox = inbox.files.filter((f) => !f.skipped).length))
        .catch(() => (this.inbox = 0));
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) this.loggedOut = true;
      else toasts.error(e, 'Cannot reach carrel');
    }
  }

  changed() {
    this.version++;
    this.refresh();
  }

  get semantic() {
    const e = this.info?.embeddings;
    return e && 'model' in e ? e : null;
  }
}

export const app = new App();
