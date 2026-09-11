import { createI18n } from "vue-i18n";
import enUS from "./en-US.json";
import enUSComments from "./en-US-comments.json";

// ==================== 语言注册表（新增语言只改这里 + 创建翻译文件） ====================

export interface LocaleEntry {
  /** 语言代码 */
  code: string;
  /** 国旗 emoji */
  flag: string;
  /** 原生显示名称 */
  label: string;
  /** navigator.language 前缀匹配规则 */
  match: (lang: string) => boolean;
  /** 是否为从右向左书写的语言 */
  rtl?: boolean;
}

// 顺序按 ISO 639-1 语言代码字母序（世界通用顺序）；中文按地区代码细分
export const localeEntries: LocaleEntry[] = [
  { code: "en-US", flag: "🇺🇸", label: "English", match: () => true },
];

/** locale code → entry 快速查找 */
const localeMap = new Map(localeEntries.map((e) => [e.code, e]));

// ==================== 工具函数 ====================

/** 应用固定使用英文 */
export const resolveLocale = (_locale: string): string => "en-US";

// ==================== i18n 实例 ====================

const defaultLocale = "en-US";

const enUSMessages = {
  ...enUS,
  settings: { ...enUS.settings, ...enUSComments.settings },
  detail: { ...enUS.detail, ...enUSComments.detail },
};

const i18n = createI18n({
  legacy: false,
  locale: defaultLocale,
  fallbackLocale: "en-US",
  messages: {
    "en-US": enUSMessages,
  },
});

/** 根据 locale code 返回文档书写方向 */
const getDirection = (code: string): "rtl" | "ltr" => (localeMap.get(code)?.rtl ? "rtl" : "ltr");

/** 切换语言（供 settings store 调用） */
export const setI18nLocale = (locale: string) => {
  const resolved = resolveLocale(locale);
  (i18n.global.locale as unknown as { value: string }).value = resolved;
  document.documentElement.lang = resolved;
  document.documentElement.dir = getDirection(resolved);
};

// 初始化时同步 html lang 和 dir
document.documentElement.lang = defaultLocale;
document.documentElement.dir = getDirection(defaultLocale);

export default i18n;
