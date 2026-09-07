<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

interface PluginInfo {
  id: string;
  name: string;
  desc: string;
  /** 用于检测是否已安装的文件相对路径（相对于插件目录） */
  checkFile: string;
  /** 插件 zip 下载地址 */
  downloadUrl: string;
  installed: boolean;
  installing: boolean;
  uninstalling: boolean;
}

const plugins = ref<PluginInfo[]>([
  {
    id: "cookie-unlock",
    name: "ChromeCookieUnlock",
    desc: t("plugins.cookieUnlockDesc"),
    checkFile: "yt_dlp_plugins/postprocessor/chrome_cookie_unlock.py",
    downloadUrl:
      "https://github.com/seproDev/yt-dlp-ChromeCookieUnlock/releases/download/v2024.04.29/yt-dlp-ChromeCookieUnlock.zip",
    installed: false,
    installing: false,
    uninstalling: false,
  },
]);

/** 检查所有插件的安装状态 */
const checkAllStatus = async () => {
  for (const plugin of plugins.value) {
    try {
      plugin.installed = await invoke<boolean>("check_plugin_installed", {
        filePath: plugin.checkFile,
      });
    } catch {
      plugin.installed = false;
    }
  }
};

/** 安装插件 */
const handleInstall = async (plugin: PluginInfo) => {
  plugin.installing = true;
  try {
    await invoke("install_plugin", { url: plugin.downloadUrl });
    plugin.installed = true;
    window.$message.success(t("plugins.installSuccess"));
  } catch (e: unknown) {
    window.$message.error(t("common.downloadFailed", { e }));
  } finally {
    plugin.installing = false;
  }
};

/** 卸载插件 */
const handleUninstall = async (plugin: PluginInfo) => {
  plugin.uninstalling = true;
  try {
    await invoke("uninstall_plugin", { filePath: plugin.checkFile });
    plugin.installed = false;
    window.$message.success(t("plugins.uninstallSuccess"));
  } catch (e: unknown) {
    window.$message.error(String(e));
  } finally {
    plugin.uninstalling = false;
  }
};

onMounted(() => {
  checkAllStatus();
});
</script>

<template>
  <n-flex vertical :size="12">
    <n-flex align="center" :size="8">
      <n-button strong secondary size="small" @click="$router.back()">
        <template #icon>
          <n-icon><icon-mdi-arrow-left /></n-icon>
        </template>
        {{ $t("common.back") }}
      </n-button>
      <n-text strong style="font-size: 15px">{{ $t("plugins.title") }}</n-text>
    </n-flex>

    <n-text depth="3" style="font-size: 13px">
      {{ $t("plugins.pageDesc") }}
    </n-text>

    <n-card v-for="plugin in plugins" :key="plugin.id" size="small">
      <n-flex align="center" :size="12" :wrap="false">
        <n-flex vertical :size="2" style="flex: 1; min-width: 0">
          <n-flex align="center" :size="8">
            <n-text strong>{{ plugin.name }}</n-text>
            <n-tag v-if="plugin.installed" size="small" round :bordered="false" type="success">
              {{ $t("settings.installed") }}
            </n-tag>
            <n-tag v-else size="small" round :bordered="false" type="warning">
              {{ $t("settings.notInstalled") }}
            </n-tag>
          </n-flex>
          <n-text depth="3" style="font-size: 12px">
            {{ plugin.desc }}
          </n-text>
        </n-flex>
        <n-button
          v-if="!plugin.installed"
          size="small"
          type="primary"
          :loading="plugin.installing"
          @click="handleInstall(plugin)"
        >
          <template #icon>
            <n-icon><icon-mdi-download-outline /></n-icon>
          </template>
          {{ $t("plugins.install") }}
        </n-button>
        <n-button
          v-else
          size="small"
          type="error"
          secondary
          :loading="plugin.uninstalling"
          @click="handleUninstall(plugin)"
        >
          <template #icon>
            <n-icon><icon-mdi-delete-outline /></n-icon>
          </template>
          {{ $t("plugins.uninstall") }}
        </n-button>
      </n-flex>
    </n-card>
  </n-flex>
</template>
