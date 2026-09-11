<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useI18n } from "vue-i18n";
import type { CloudFolder, CloudLibrary, CloudVideo } from "@/types";
import { useSettingStore } from "@/stores/setting";

const DEFAULT_R2_BUCKET = "yt-dlp-gui-videos-20260906";

const { t } = useI18n();
const settingStore = useSettingStore();

const folders = ref<CloudFolder[]>([]);
const selectedFolderPath = ref("");
const searchQuery = ref("");
const isLoading = ref(false);
const error = ref("");

const r2Config = computed(() => {
  const accountId = settingStore.r2AccountId.trim();
  return {
    accountId,
    accessKeyId: settingStore.r2AccessKeyId.trim(),
    secretAccessKey: settingStore.r2SecretAccessKey.trim(),
    endpoint:
      settingStore.r2Endpoint.trim() ||
      (accountId ? `https://${accountId}.r2.cloudflarestorage.com` : ""),
    bucket: settingStore.r2Bucket.trim() || DEFAULT_R2_BUCKET,
    publicBaseUrl: settingStore.r2PublicBaseUrl.trim(),
  };
});

const credentialsReady = computed(() =>
  Boolean(
    r2Config.value.accountId &&
    r2Config.value.accessKeyId &&
    r2Config.value.secretAccessKey &&
    r2Config.value.endpoint,
  ),
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
  if (!timestamp) return t("cloudFiles.unknownDate");
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(timestamp * 1000),
  );
};

const notifyError = (message: unknown) => {
  const value = String(message || t("cloudFiles.loadFailed"));
  const messageText = value.startsWith("err_") ? t("cloudFiles.operationFailed") : value;
  error.value = messageText;
  window.$message.error(messageText);
};

const loadLibrary = async () => {
  isLoading.value = true;
  error.value = "";
  try {
    const library = await invoke<CloudLibrary>("list_r2_videos", r2Config.value);
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

const copyUrl = async (url: string) => {
  try {
    await writeText(url);
    window.$message.success(t("cloudFiles.urlCopied"));
  } catch {
    window.$message.error(t("cloudFiles.copyFailed"));
  }
};

const openVideo = async (video: CloudVideo) => {
  try {
    await openUrl(video.url);
  } catch (err) {
    notifyError(err);
  }
};

const deleteVideo = (video: CloudVideo) => {
  window.$dialog.warning({
    title: t("cloudFiles.deleteVideoTitle"),
    content: t("cloudFiles.deleteVideoContent", { name: video.name }),
    positiveText: t("common.remove"),
    negativeText: t("common.cancel"),
    onPositiveClick: async () => {
      try {
        await invoke("delete_r2_object", { key: video.key, ...r2Config.value });
        await loadLibrary();
        window.$message.success(t("cloudFiles.deleted"));
      } catch (err) {
        notifyError(err);
      }
    },
  });
};

const deleteFolder = (folder: CloudFolder) => {
  window.$dialog.warning({
    title: t("cloudFiles.deleteFolderTitle"),
    content: t("cloudFiles.deleteFolderContent", { name: folder.name }),
    positiveText: t("common.remove"),
    negativeText: t("common.cancel"),
    onPositiveClick: async () => {
      try {
        for (const video of folder.videos) {
          await invoke("delete_r2_object", { key: video.key, ...r2Config.value });
        }
        await loadLibrary();
        window.$message.success(t("cloudFiles.deleted"));
      } catch (err) {
        notifyError(err);
      }
    },
  });
};

onMounted(() => {
  void loadLibrary();
});
</script>

<template>
  <div class="cloud-files-page">
    <n-flex vertical :size="16" style="height: 100%">
      <n-flex align="center" justify="space-between" wrap>
        <div>
          <n-h2 prefix="bar" align-text style="margin: 0">{{ $t("cloudFiles.title") }}</n-h2>
          <n-text depth="3">{{ $t("cloudFiles.subtitle") }}</n-text>
        </div>
        <n-flex align="center" :size="8" wrap>
          <n-button size="small" :loading="isLoading" @click="loadLibrary">
            <template #icon>
              <n-icon><icon-mdi-refresh /></n-icon>
            </template>
            {{ $t("common.refresh") }}
          </n-button>
        </n-flex>
      </n-flex>

      <n-alert v-if="error" type="error" :title="$t('cloudFiles.loadFailed')">
        {{ error }}
      </n-alert>
      <n-alert v-if="!credentialsReady" type="warning" :title="$t('cloudFiles.configurationTitle')">
        {{ $t("cloudFiles.configurationContent") }}
      </n-alert>

      <div class="root-row">
        <n-icon size="18"><icon-mdi-cloud-outline /></n-icon>
        <n-text depth="3" class="root-path">
          {{ r2Config.bucket }}
          <span class="endpoint-label">
            · {{ r2Config.publicBaseUrl || $t("cloudFiles.privateSignedUrls") }}
          </span>
        </n-text>
        <n-button
          text
          size="tiny"
          :disabled="!r2Config.publicBaseUrl"
          @click="copyUrl(r2Config.publicBaseUrl)"
        >
          <template #icon>
            <n-icon><icon-mdi-content-copy /></n-icon>
          </template>
        </n-button>
      </div>

      <div class="library-layout">
        <n-card size="small" class="folders-panel" :title="$t('cloudFiles.folders')">
          <template #header-extra>
            <n-tag size="small" round>{{ folders.length }}</n-tag>
          </template>
          <n-input
            v-model:value="searchQuery"
            clearable
            size="small"
            :placeholder="$t('cloudFiles.searchFolders')"
          >
            <template #prefix>
              <n-icon><icon-mdi-magnify /></n-icon>
            </template>
          </n-input>
          <n-scrollbar class="folder-list">
            <n-empty
              v-if="!isLoading && filteredFolders.length === 0"
              size="small"
              :description="$t('cloudFiles.noFolders')"
            />
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
                <template #icon>
                  <n-icon><icon-mdi-folder /></n-icon>
                </template>
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
            <n-button
              text
              size="small"
              type="error"
              :disabled="selectedFolder.videos.length === 0"
              @click="deleteFolder(selectedFolder)"
            >
              <template #icon>
                <n-icon><icon-mdi-delete-outline /></n-icon>
              </template>
            </n-button>
          </template>

          <n-empty
            v-if="selectedFolder.videos.length === 0"
            :description="$t('cloudFiles.noVideos')"
          />
          <div v-else class="video-grid">
            <n-card
              v-for="video in selectedFolder.videos"
              :key="video.key"
              size="small"
              class="video-card"
              content-style="padding: 0"
            >
              <div class="video-cover" @click="openVideo(video)">
                <video
                  class="video-preview"
                  :src="video.url"
                  preload="metadata"
                  muted
                  playsinline
                />
                <div class="play-overlay">
                  <n-icon size="30"><icon-mdi-play /></n-icon>
                </div>
                <div
                  class="cloud-badge"
                  :title="$t('cloudFiles.openVideo')"
                  @click.stop="openVideo(video)"
                >
                  <n-icon size="13"><icon-mdi-cloud-check /></n-icon>
                  <span>R2</span>
                </div>
              </div>
              <div class="video-info">
                <n-ellipsis :line-clamp="2" :tooltip="false">{{ video.name }}</n-ellipsis>
                <n-text depth="3" class="video-meta">
                  {{ formatSize(video.size) }} · {{ formatDate(video.modified) }}
                </n-text>
                <n-flex :size="4" justify="end" align="center">
                  <n-button
                    text
                    size="tiny"
                    type="primary"
                    :title="$t('cloudFiles.openVideo')"
                    @click="openVideo(video)"
                  >
                    <template #icon>
                      <n-icon><icon-mdi-open-in-new /></n-icon>
                    </template>
                  </n-button>
                  <n-button
                    text
                    size="tiny"
                    type="success"
                    :title="$t('cloudFiles.copyUrl')"
                    @click="copyUrl(video.url)"
                  >
                    <template #icon>
                      <n-icon><icon-mdi-link-variant /></n-icon>
                    </template>
                  </n-button>
                  <n-button
                    text
                    size="tiny"
                    type="error"
                    :title="$t('common.remove')"
                    @click="deleteVideo(video)"
                  >
                    <template #icon>
                      <n-icon><icon-mdi-delete-outline /></n-icon>
                    </template>
                  </n-button>
                </n-flex>
              </div>
            </n-card>
          </div>
        </n-card>
        <n-card v-else size="small" class="videos-panel empty-panel">
          <n-empty :description="$t('cloudFiles.selectFolder')" />
        </n-card>
      </div>
    </n-flex>
  </div>
</template>

<style scoped lang="scss">
.cloud-files-page {
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

.endpoint-label {
  font-family: inherit;
  opacity: 0.7;
}

.library-layout {
  display: grid;
  grid-template-columns: minmax(180px, 240px) minmax(0, 1fr);
  gap: 12px;
  flex: 1;
  min-height: 0;
}

.folders-panel,
.videos-panel {
  min-height: 0;
}

.folders-panel :deep(.n-card__content),
.videos-panel :deep(.n-card__content) {
  min-height: 0;
  height: 100%;
}

.folder-list {
  height: calc(100% - 42px);
  margin-top: 10px;
}

.folder-item {
  text-align: left;
}

.folder-item.selected {
  color: var(--n-primary-color);
  background-color: var(--n-action-color);
}

.folder-name {
  flex: 1;
}

.video-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 12px;
}

.video-card {
  overflow: hidden;
}

.video-cover {
  position: relative;
  aspect-ratio: 16 / 9;
  overflow: hidden;
  cursor: pointer;
  background: var(--n-color-modal);
}

.video-preview {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.play-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  opacity: 0;
  background: rgb(0 0 0 / 40%);
  transition: opacity 0.2s;
}

.video-cover:hover .play-overlay {
  opacity: 1;
}

.cloud-badge {
  position: absolute;
  top: 8px;
  right: 8px;
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 3px 6px;
  color: white;
  font-size: 11px;
  border-radius: 4px;
  background: rgb(0 0 0 / 55%);
}

.video-info {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px;
}

.video-meta {
  font-size: 12px;
}

.empty-panel {
  display: flex;
  align-items: center;
  justify-content: center;
}

@media (max-width: 700px) {
  .library-layout {
    grid-template-columns: 1fr;
  }

  .folders-panel {
    max-height: 220px;
  }
}
</style>
