// Short messages in the corner: what got added, what failed.

export interface Toast {
  id: number;
  kind: 'info' | 'error' | 'success';
  title: string;
  body?: string;
}

class Toasts {
  list = $state<Toast[]>([]);
  #next = 0;

  show(kind: Toast['kind'], title: string, body?: string) {
    const id = this.#next++;
    this.list.push({ id, kind, title, body });
    setTimeout(() => this.dismiss(id), kind === 'error' ? 9000 : 5000);
  }

  dismiss(id: number) {
    this.list = this.list.filter((t) => t.id !== id);
  }

  error(e: unknown, title = 'Something went wrong') {
    this.show('error', title, e instanceof Error ? e.message : String(e));
  }
}

export const toasts = new Toasts();
