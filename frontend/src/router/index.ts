import { createRouter, createWebHistory } from 'vue-router'

import LoginView from '../views/LoginView.vue'
import DesktopLayout from '../components/layout/DesktopLayout.vue'
import DesktopView from '../views/DesktopView.vue'
import http from '@/api'
import type { AuthBody } from '@/api/interface'

const router = createRouter({
  history: createWebHistory('/'),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: LoginView,
    },
    {
      path: '/upgrade-progress',
      name: 'upgrade-progress',
      component: () => import('../apps/views/UpgradeProgressView.vue'),
    },
    {
      path: '/',
      component: DesktopLayout,
      children: [
        {
          path: '', // 映射到 /
          name: 'desktop',
          component: DesktopView,
          meta: { requiresAuth: true },
        },
      ],
    },
    // 捕获所有未匹配路径，跳转到首页
    {
      path: '/:pathMatch(.*)*',
      redirect: '/',
    },
  ],
})

let isLoggedIn = false
let hasChecked = false

/**
 * @description 路由守卫：处理登录拦截逻辑
 */
router.beforeEach(async (to, from) => {
  // 只有当用户从登录页去往非登录页时（可能刚刚登录成功），才在下一次强制重新请求最新的 session
  if (from.path === '/login' && to.path !== '/login') {
    hasChecked = false
  }

  // 1. 如果去往需要授权的页面
  if (to.meta.requiresAuth) {
    if (!hasChecked) {
      const res = await http.get<AuthBody>('/auth/me')
      isLoggedIn = res.success
      hasChecked = true
    }
    if (!isLoggedIn) {
      return '/login'
    }
  }

  // 2. 如果去往登录页
  if (to.path === '/login') {
    if (!hasChecked) {
      const res = await http.get<AuthBody>('/auth/me')
      isLoggedIn = res.success
      hasChecked = true
    }
    if (isLoggedIn) {
      return '/'
    }
  }
})

export function resetAuthState() {
  isLoggedIn = false
  hasChecked = false
}

export default router
