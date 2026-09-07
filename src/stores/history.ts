import { defineStore } from "pinia";

const MAX_HISTORY = 50;

export interface HistoryItem {
  url: string;
  title: string;
  time: number;
}

export const useHistoryStore = defineStore(
  "history",
  () => {
    const items = ref<HistoryItem[]>([]);

    const urls = computed(() => items.value.map((i) => i.url));

    /** 添加一条成功获取过的链接（去重，最新在前） */
    const add = (url: string, title?: string) => {
      const trimmed = url.trim();
      if (!trimmed) return;
      const idx = items.value.findIndex((i) => i.url === trimmed);
      if (idx !== -1) items.value.splice(idx, 1);
      items.value.unshift({ url: trimmed, title: title || trimmed, time: Date.now() });
      if (items.value.length > MAX_HISTORY) items.value.length = MAX_HISTORY;
    };

    /** 删除单条记录 */
    const remove = (url: string) => {
      const idx = items.value.findIndex((i) => i.url === url);
      if (idx !== -1) items.value.splice(idx, 1);
    };

    /** 清空所有历史 */
    const clear = () => {
      items.value = [];
    };

    return { items, urls, add, remove, clear };
  },
  {
    persist: true,
  },
);
