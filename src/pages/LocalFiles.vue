<script setup lang="ts">
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useI18n } from "vue-i18n";
import type { LocalFolder, LocalLibrary, LocalVideo } from "@/types";
import { useSettingStore } from "@/stores/setting";

const { t } = useI18n();
const settingStore = useSettingStore();

const rootPath = ref(settingStore.downloadDir);
const folders = ref<LocalFolder[]>([]);
const selectedFolderPath = ref("");
const searchQuery = ref("");
const isLoading = ref(false);
const error = ref("");
const uploading = ref<Record<string, boolean>>({});

const R2_URLS_STORAGE_KEY = "r2-uploaded-urls-v1";

const loadStoredR2Urls = (): Record<string, string> => {
  try {
    const raw = localStorage.getItem(R2_URLS_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, string>;
    if (!parsed || typeof parsed !== "object") return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(
        ([key, value]) =>
          typeof key === "string" && typeof value === "string" && value.startsWith("http"),
      ),
    );
  } catch {
    return {};
  }
};

const uploadedUrls = ref<Record<string, string>>(loadStoredR2Urls());

watch(
  uploadedUrls,
  (value) => {
    try {
      localStorage.setItem(R2_URLS_STORAGE_KEY, JSON.stringify(value));
    } catch {
      // Ignore storage quota errors; URLs remain available for this session.
    }
  },
  { deep: true },
);

const selectedFolder = computed(
  () => folders.value.find((folder) => folder.path === selectedFolderPath.value) ?? null,
);

const filteredFolders = computed(() => {
  const query = searchQuery.value.trim().toLocaleLowerCase();
  if (!query) return folders.value;
  return folders.value.filter((folder) => folder.name.toLocaleLowerCase().includes(query));
});

const formatSize = (bytes: number) => {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = -1;
  do {
    value /= 1024;
    unit += 1;
  } while (value >= 1024 && unit < units.length - 1);
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
};

const formatDate = (timestamp: number | null) => {
  if (!timestamp) return t("localFiles.unknownDate");
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(timestamp * 1000),
  );
};

const previewSource = (path: string) => convertFileSrc(path);

const notifyError = (message: unknown) => {
  const value = String(message || t("localFiles.loadFailed"));
  error.value = value;
  window.$message.error(value.startsWith("err_") ? t("localFiles.operationFailed") : value);
};

const loadLibrary = async (path = rootPath.value) => {
  const normalized = path.trim();
  if (!normalized) {
    folders.value = [];
    selectedFolderPath.value = "";
    error.value = "";
    return;
  }

  isLoading.value = true;
  error.value = "";
  try {
    const library = await invoke<LocalLibrary>("scan_local_files", { rootPath: normalized });
    rootPath.value = library.root_path;
    folders.value = library.folders;
    if (!library.folders.some((folder) => folder.path === selectedFolderPath.value)) {
      selectedFolderPath.value = library.folders[0]?.path ?? "";
    }
  } catch (err) {
    folders.value = [];
    selectedFolderPath.value = "";
    notifyError(err);
  } finally {
    isLoading.value = false;
  }
};

const chooseRoot = async () => {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("localFiles.chooseRoot"),
    });
    if (!selected) return;
    rootPath.value = selected as string;
    await loadLibrary();
  } catch (err) {
    notifyError(err);
  }
};

const copyText = async (value: string, successMessage: string, failureMessage: string) => {
  try {
    await writeText(value);
    window.$message.success(successMessage);
    return true;
  } catch {
    window.$message.error(failureMessage);
    return false;
  }
};

const copyPath = async (path: string) => {
  await copyText(path, t("localFiles.pathCopied"), t("localFiles.copyFailed"));
};

const copyR2Url = async (video: LocalVideo) => {
  const url = uploadedUrls.value[video.path];
  if (!url) return;
  await copyText(url, t("localFiles.r2UrlCopied"), t("localFiles.copyFailed"));
};

const openFolder = async (folder: LocalFolder) => {
  try {
    await revealItemInDir(folder.path);
  } catch (err) {
    notifyError(err);
  }
};

const playVideo = async (video: LocalVideo) => {
  try {
    await openPath(video.path);
  } catch (err) {
    notifyError(err);
  }
};

const deleteVideo = (video: LocalVideo) => {
  window.$dialog.warning({
    title: t("localFiles.deleteVideoTitle"),
    content: t("localFiles.deleteVideoContent", { name: video.name }),
    positiveText: t("common.remove"),
    negativeText: t("common.cancel"),
    onPositiveClick: async () => {
      try {
        await invoke("delete_local_file", { rootPath: rootPath.value, filePath: video.path });
        delete uploadedUrls.value[video.path];
        await loadLibrary();
        window.$message.success(t("localFiles.deleted"));
      } catch (err) {
        notifyError(err);
      }
    },
  });
};

const uploadVideo = async (video: LocalVideo, folder: LocalFolder) => {
  if (uploadedUrls.value[video.path]) {
    await copyText(
      uploadedUrls.value[video.path],
      t("localFiles.r2UrlCopied"),
      t("localFiles.copyFailed"),
    );
    return;
  }
  uploading.value[video.path] = true;
  try {
    const url = await invoke<string>("upload_local_video_to_r2", {
      filePath: video.path,
      objectKey: `${folder.name}/${video.name}`,
      accountId: settingStore.r2AccountId,
      accessKeyId: settingStore.r2AccessKeyId,
      secretAccessKey: settingStore.r2SecretAccessKey,
      endpoint: settingStore.r2Endpoint,
      bucket: settingStore.r2Bucket,
      publicBaseUrl: settingStore.r2PublicBaseUrl,
    });
    uploadedUrls.value[video.path] = url;
    await copyText(url, t("localFiles.r2Uploaded"), t("localFiles.copyFailed"));
  } catch (err) {
    notifyError(err);
  } finally {
    uploading.value[video.path] = false;
  }
};

const uploadFolder = async (folder: LocalFolder) => {
  for (const video of folder.videos) {
    await uploadVideo(video, folder);
  }
};

const deleteFolder = (folder: LocalFolder) => {
  window.$dialog.warning({
    title: t("localFiles.deleteFolderTitle"),
    content: t("localFiles.deleteFolderContent", { name: folder.name }),
    positiveText: t("common.remove"),
    negativeText: t("common.cancel"),
    onPositiveClick: async () => {
      try {
        await invoke("delete_local_folder", { rootPath: rootPath.value, folderPath: folder.path });
        for (const video of folder.videos) {
          delete uploadedUrls.value[video.path];
        }
        await loadLibrary();
        window.$message.success(t("localFiles.deleted"));
      } catch (err) {
        notifyError(err);
      }
    },
  });
};

watch(
  () => settingStore.downloadDir,
  (value) => {
    if (!rootPath.value && value) {
      rootPath.value = value;
      void loadLibrary(value);
    }
  },
);

onMounted(() => {
  void loadLibrary();
});
</script>

<template>
  <div class="local-files-page">
    <n-flex vertical :size="16" style="height: 100%">
      <n-flex align="center" justify="space-between" wrap>
        <div>
          <n-h2 prefix="bar" align-text style="margin: 0">{{ $t("localFiles.title") }}</n-h2>
          <n-text depth="3">{{ $t("localFiles.subtitle") }}</n-text>
        </div>
        <n-flex align="center" :size="8" wrap>
          <n-button size="small" :loading="isLoading" @click="loadLibrary()">
            <template #icon><n-icon><icon-mdi-refresh /></n-icon></template>
            {{ $t("common.refresh") }}
          </n-button>
          <n-button type="primary" size="small" @click="chooseRoot">
            <template #icon><n-icon><icon-mdi-folder-open-outline /></n-icon></template>
            {{ $t("localFiles.chooseRoot") }}
          </n-button>
        </n-flex>
      </n-flex>

      <n-alert v-if="error" type="error" :title="$t('localFiles.loadFailed')">
        {{ error }}
      </n-alert>
      <n-alert v-if="!rootPath" type="warning" :title="$t('localFiles.noRootTitle')">
        {{ $t("localFiles.noRootContent") }}
      </n-alert>

      <div class="root-row">
        <n-icon size="18"><icon-mdi-folder-outline /></n-icon>
        <n-text depth="3" class="root-path">{{ rootPath || $t("localFiles.notSet") }}</n-text>
        <n-button v-if="rootPath" text size="tiny" @click="copyPath(rootPath)">
          <template #icon><n-icon><icon-mdi-content-copy /></n-icon></template>
        </n-button>
      </div>

      <div class="library-layout">
        <n-card size="small" class="folders-panel" :title="$t('localFiles.folders')">
          <template #header-extra>
            <n-tag size="small" round>{{ folders.length }}</n-tag>
          </template>
          <n-input v-model:value="searchQuery" clearable size="small" :placeholder="$t('localFiles.searchFolders')">
            <template #prefix><n-icon><icon-mdi-magnify /></n-icon></template>
          </n-input>
          <n-scrollbar class="folder-list">
            <n-empty v-if="!isLoading && filteredFolders.length === 0" size="small" :description="$t('localFiles.noFolders')" />
            <n-skeleton v-else-if="isLoading" text :repeat="5" />
            <n-flex v-else vertical :size="4">
              <n-button
                v-for="folder in filteredFolders"
                :key="folder.path"
                class="folder-item"
                :class="{ selected: selectedFolder?.path === folder.path }"
                quaternary
                block
                justify="start"
                @click="selectedFolderPath = folder.path"
              >
                <template #icon><n-icon><icon-mdi-folder /></n-icon></template>
                <span class="folder-name">{{ folder.name }}</span>
                <n-tag size="tiny" round>{{ folder.videos.length }}</n-tag>
              </n-button>
            </n-flex>
          </n-scrollbar>
        </n-card>

        <n-card v-if="selectedFolder" size="small" class="videos-panel">
          <template #header>
            <n-flex align="center" :size="8" style="min-width: 0">
              <n-icon size="22"><icon-mdi-folder-open /></n-icon>
              <n-text strong class="selected-folder-name">{{ selectedFolder.name }}</n-text>
              <n-tag size="small" round>{{ selectedFolder.videos.length }}</n-tag>
            </n-flex>
          </template>
          <template #header-extra>
            <n-flex :size="4" align="center">
              <n-tag v-if="selectedFolder.videos.some((video) => uploadedUrls[video.path])" size="small" round type="success" :title="$t('localFiles.r2Available')">
                R2 {{ selectedFolder.videos.filter((video) => uploadedUrls[video.path]).length }}/{{ selectedFolder.videos.length }}
              </n-tag>
              <n-button text size="small" type="primary" :title="$t('localFiles.uploadToR2')" :loading="selectedFolder.videos.some((video) => uploading[video.path])" :disabled="selectedFolder.videos.length === 0" @click="uploadFolder(selectedFolder)">
                <template #icon><n-icon><icon-simple-icons-cloudflare /></n-icon></template>
              </n-button>
              <n-button text size="small" @click="openFolder(selectedFolder)">
                <template #icon><n-icon><icon-mdi-folder-open-outline /></n-icon></template>
              </n-button>
              <n-button v-if="selectedFolder.path !== rootPath" text size="small" type="error" @click="deleteFolder(selectedFolder)">
                <template #icon><n-icon><icon-mdi-delete-outline /></n-icon></template>
              </n-button>
            </n-flex>
          </template>

          <n-empty v-if="selectedFolder.videos.length === 0" :description="$t('localFiles.noVideos')" />
          <div v-else class="video-grid">
            <n-card v-for="video in selectedFolder.videos" :key="video.path" size="small" class="video-card" content-style="padding: 0">
              <div class="video-cover" @click="playVideo(video)">
                <video
                  class="video-preview"
                  :src="previewSource(video.path)"
                  :poster="video.thumbnail ? previewSource(video.thumbnail) : undefined"
                  preload="metadata"
                  muted
                  playsinline
                />
                <div class="play-overlay"><n-icon size="30"><icon-mdi-play /></n-icon></div>
                <n-tooltip v-if="uploadedUrls[video.path]" trigger="hover">
                  <template #trigger>
                    <div class="cloud-badge" :title="$t('localFiles.copyR2Url')" @click.stop="copyR2Url(video)">
                      <n-icon size="13"><icon-mdi-cloud-check /></n-icon>
                      <span>R2</span>
                    </div>
                  </template>
                  {{ $t("localFiles.r2Available") }}
                </n-tooltip>
              </div>
              <div class="video-info">
                <n-ellipsis :line-clamp="2" :tooltip="false">{{ video.name }}</n-ellipsis>
                <n-text depth="3" class="video-meta">{{ formatSize(video.size) }} · {{ formatDate(video.modified) }}</n-text>
                <n-flex :size="4" justify="end" align="center">
                  <n-tag v-if="uploadedUrls[video.path]" size="tiny" round type="success" :title="$t('localFiles.r2Available')">R2</n-tag>
                  <n-button text size="tiny" :type="uploadedUrls[video.path] ? 'success' : 'primary'" :loading="uploading[video.path]" :title="uploadedUrls[video.path] ? $t('localFiles.copyR2Url') : $t('localFiles.uploadToR2')" @click="uploadVideo(video, selectedFolder)">
                    <template #icon><n-icon><icon-simple-icons-cloudflare /></n-icon></template>
                  </n-button>
                  <n-button v-if="uploadedUrls[video.path]" text size="tiny" type="success" :title="$t('localFiles.copyR2Url')" @click="copyR2Url(video)">
                    <template #icon><n-icon><icon-mdi-link-variant /></n-icon></template>
                  </n-button>
                  <n-button text size="tiny" @click="copyPath(video.path)">
                    <template #icon><n-icon><icon-mdi-content-copy /></n-icon></template>
                  </n-button>
                  <n-button text size="tiny" type="error" @click="deleteVideo(video)">
                    <template #icon><n-icon><icon-mdi-delete-outline /></n-icon></template>
                  </n-button>
                </n-flex>
              </div>
            </n-card>
          </div>
        </n-card>
        <n-card v-else size="small" class="videos-panel empty-panel">
          <n-empty :description="$t('localFiles.selectFolder')" />
        </n-card>
      </div>
    </n-flex>
  </div>
</template>

<style scoped lang="scss">
.local-files-page {
  height: 100%;
  min-height: 0;
}

.root-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  padding: 8px 12px;
  border: 1px dashed var(--n-border-color);
  border-radius: 8px;
}

.root-path,
.folder-name,
.selected-folder-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.root-path {
  flex: 1;
  font-family: monospace;
  font-size: 12px;
}

.library-layout {
  display: grid;
  grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
  gap: 12px;
  min-height: 0;
  flex: 1;
}

.folders-panel,
.videos-panel {
  min-height: 0;
}

.folders-panel :deep(.n-card__content) {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 12px;
}

.folder-list {
  flex: 1;
  min-height: 0;
  margin-top: 10px;
}

.folder-item {
  min-width: 0;
  text-align: left;
}

.folder-item :deep(.n-button__content) {
  width: 100%;
  min-width: 0;
  justify-content: flex-start;
}

.folder-item :deep(.n-button__icon) {
  flex-shrink: 0;
  margin-right: 8px;
}

.folder-item.selected {
  color: var(--n-primary-color);
  background: var(--n-color-target);
}

.folder-name {
  flex: 1;
  text-align: left;
}

.selected-folder-name {
  max-width: 360px;
}

.video-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 12px;
  overflow-y: auto;
  max-height: 100%;
}

.video-card {
  overflow: hidden;
}

.video-cover {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  aspect-ratio: 16 / 9;
  color: var(--n-primary-color);
  background: linear-gradient(135deg, rgba(24, 160, 88, 0.14), rgba(24, 160, 88, 0.03));
  cursor: pointer;
  overflow: hidden;
}

.video-preview {
  width: 100%;
  height: 100%;
  object-fit: cover;
  background: transparent;
}

.play-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  background: rgba(0, 0, 0, 0.28);
  opacity: 0;
  transition: opacity 0.2s;
}

.video-cover:hover .play-overlay {
  opacity: 1;
}

.cloud-badge {
  position: absolute;
  top: 6px;
  right: 6px;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 2px 8px;
  font-size: 11px;
  font-weight: 600;
  line-height: 1.4;
  color: #fff;
  background: rgba(24, 160, 88, 0.92);
  border-radius: 999px;
  cursor: pointer;
}

.cloud-badge:hover {
  background: rgba(24, 160, 88, 1);
}

.video-info {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px;
}

.video-meta {
  font-size: 11px;
}

.empty-panel {
  display: flex;
  align-items: center;
  justify-content: center;
}

@media (max-width: 700px) {
  .library-layout {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(150px, 220px) minmax(280px, 1fr);
  }
}
</style>
