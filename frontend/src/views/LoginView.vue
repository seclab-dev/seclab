<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import type { AuthBody } from '@/api/interface'
import http from '@/api'
import { versionStaticAsset } from '@/utils/static-assets'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const loginLogo = versionStaticAsset('/images/seclab-login-logo.png')
const username = ref('')
const password = ref('')
const errorMessage = ref('')
const router = useRouter()

/**
 * @description 处理登录逻辑，向后端 API 发送请求
 */
async function handleLogin() {
  errorMessage.value = ''
  if (username.value === '' || password.value === '') {
    errorMessage.value = t('login.error.emptyCredentials')
    return
  }

  const res = await http.post<AuthBody>('/auth/login', {
    username: username.value,
    password: password.value,
  })

  if (res.success) {
    router.push('/')
  } else {
    errorMessage.value = res.message || t('login.error.loginFailed')
  }
}
</script>

<template>
  <div class="login-container" data-page="login">
    <div class="login-card" data-ui="login-card" aria-labelledby="login-title">
      <div class="login-header" data-slot="header">
        <div class="login-logo-wrapper">
          <img :src="loginLogo" alt="SecLab Logo" class="login-logo" />
        </div>
        <h1 id="login-title" class="brand-title">{{ $t('login.brand') }}</h1>
        <p class="brand-slogan">{{ $t('login.slogan') }}</p>
      </div>

      <form
        @submit.prevent="handleLogin"
        class="login-form"
        data-ui="login-form"
        autocomplete="off"
      >
        <div class="form-group">
          <label for="username">{{ $t('login.username') }}</label>
          <div class="input-wrapper">
            <input
              id="username"
              type="text"
              v-model="username"
              class="input"
              :placeholder="$t('login.usernamePlaceholder')"
              autocomplete="off"
              required
            />
          </div>
        </div>
        <div class="form-group">
          <label for="password">{{ $t('login.password') }}</label>
          <div class="input-wrapper">
            <input
              id="password"
              type="password"
              v-model="password"
              class="input"
              :placeholder="$t('login.passwordPlaceholder')"
              autocomplete="new-password"
              required
            />
          </div>
        </div>

        <div v-if="errorMessage" class="error-box" role="alert">
          <span class="error-icon" aria-hidden="true">!</span>
          <span class="error-text">{{ errorMessage }}</span>
        </div>

        <button type="submit" class="login-button" data-ui="login-submit">
          {{ $t('login.loginButton') }}
        </button>
      </form>

      <div class="login-footer" data-slot="footer">
        <p class="copyright">© 2026 Security Lab Platform. All Rights Reserved.</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.login-container {
  position: relative;
  width: 100vw;
  height: 100vh;
  min-height: 560px;
  overflow: hidden;
  background-color: var(--sdl-bg-canvas);
  background-image:
    linear-gradient(var(--sdl-border-subtle) 1px, transparent 1px),
    linear-gradient(90deg, var(--sdl-border-subtle) 1px, transparent 1px),
    radial-gradient(circle at 50% 26%, rgba(0, 200, 255, 0.14), transparent 34%),
    radial-gradient(circle at 18% 82%, rgba(0, 212, 180, 0.08), transparent 28%);
  background-size:
    48px 48px,
    48px 48px,
    100% 100%,
    100% 100%;
  display: flex;
  justify-content: center;
  align-items: center;
  padding: var(--sdl-space-8) var(--sdl-space-4);
}

.login-card {
  width: min(100%, 420px);
  padding: 44px 40px 36px;
  background: color-mix(in srgb, var(--sdl-bg-panel) 92%, transparent);
  border: 1px solid var(--sdl-border-strong);
  border-radius: var(--sdl-radius-lg);
  box-shadow: var(--sdl-shadow-window);
  display: flex;
  flex-direction: column;
  gap: 32px;
  position: relative;
}

.login-card::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

.login-header {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
}

.login-logo-wrapper {
  display: grid;
  place-items: center;
  width: 76px;
  height: 76px;
  margin-bottom: var(--sdl-space-6);
  border: 1px solid var(--sdl-border-brand);
  border-radius: var(--sdl-radius-lg);
  background: var(--sdl-bg-card);
  box-shadow: var(--sdl-shadow-brand);
}

.login-logo {
  height: 52px;
  width: auto;
}

.brand-title {
  margin: 0;
  font-size: var(--sdl-font-metric);
  font-weight: 600;
  color: var(--sdl-text-primary);
  letter-spacing: 0.5px;
}

.brand-slogan {
  margin: 8px 0 0;
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-muted);
}

.login-form {
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.form-group label {
  font-size: var(--sdl-font-body-sm);
  font-weight: 500;
  color: var(--sdl-text-secondary);
  margin-left: 2px;
}

.input-wrapper {
  position: relative;
}

.input {
  width: 100%;
  height: 44px;
  padding: 0 16px;
  background: var(--sdl-bg-input);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  color: var(--sdl-text-primary);
  font-size: var(--sdl-font-body);
  transition: all 0.2s ease;
  outline: none;
}

.input:focus {
  border-color: var(--sdl-primary);
  box-shadow: var(--sdl-focus-ring);
}

.input::placeholder {
  color: var(--sdl-text-muted);
  opacity: 0.4;
}

.error-box {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  background: var(--sdl-danger-soft);
  border: 1px solid var(--sdl-border-danger);
  border-radius: var(--sdl-radius-md);
  margin-top: 4px;
}

.error-icon {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border-radius: var(--sdl-radius-pill);
  background: var(--sdl-danger);
  color: var(--sdl-text-on-danger);
  font-size: var(--sdl-font-caption);
  font-weight: 700;
  line-height: 1;
}

.error-text {
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-danger);
}

.login-button {
  width: 100%;
  height: 44px;
  margin-top: 12px;
  background: var(--sdl-primary);
  color: var(--sdl-text-inverse);
  border: none;
  border-radius: var(--sdl-radius-md);
  font-size: var(--sdl-font-body);
  font-weight: 600;
  cursor: pointer;
  transition: all 0.2s ease;
}

.login-button:hover {
  background: var(--sdl-primary-hover);
}

.login-button:active {
  transform: translateY(0);
}

.login-footer {
  margin-top: 16px;
  text-align: center;
}

.copyright {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
  margin: 0;
}

@media (max-width: 768px) {
  .login-container {
    min-height: 100vh;
    padding: var(--sdl-space-6) var(--sdl-space-4);
  }

  .login-card {
    padding: 36px 28px 32px;
    gap: 28px;
  }
}
</style>
