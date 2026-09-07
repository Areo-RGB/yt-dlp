import { createApp } from "vue";
import App from "./App.vue";
// i18n
import i18n from "./locales";
// pinia
import { createPinia } from "pinia";
import piniaPluginPersistedstate from "pinia-plugin-persistedstate";
// router
import router from "./router";

// styles
import "@/styles/main.scss";
import "@/styles/transitions.scss";

// 挂载
const app = createApp(App);
// i18n
app.use(i18n);
// pinia
const pinia = createPinia();
pinia.use(piniaPluginPersistedstate);
app.use(pinia);
// router
app.use(router);

// app
app.mount("#app");
