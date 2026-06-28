<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { SecLabButton, SecLabDescriptions, SecLabDialog } from '@/components/ui'
import type { SimRule } from '@/api/modules/simulation'
import HttpRuleDetail from './details/HttpRuleDetail.vue'
import RedisRuleDetail from './details/RedisRuleDetail.vue'
import MailRuleDetail from './details/MailRuleDetail.vue'
import CredsRuleDetail from './details/CredsRuleDetail.vue'

const props = defineProps<{
  visible: boolean
  rule: SimRule | null
}>()

const emit = defineEmits<{
  close: []
  launchBrowser: [html: string | undefined]
}>()

const { t, locale } = useI18n()

const parsedConfig = computed<Record<string, unknown> | null>(() => {
  if (!props.rule) return null
  try {
    return JSON.parse(props.rule.configYaml)
  } catch {
    return null
  }
})

const protocol = computed(() => props.rule?.protocol || '')
const isRedisRule = computed(() => protocol.value === 'redis')
const isMailRule = computed(() => ['smtp', 'pop3', 'imap'].includes(protocol.value))
const isCredsRule = computed(() => ['ssh', 'ftp', 'rdp'].includes(protocol.value))
const requireAuth = computed(() => Boolean(parsedConfig.value?.require_auth))
const banner = computed(() =>
  typeof parsedConfig.value?.banner === 'string' ? parsedConfig.value.banner : '',
)
const serverHeader = computed(() =>
  typeof parsedConfig.value?.server_header === 'string' ? parsedConfig.value.server_header : '',
)

const getRuleName = (rule?: SimRule | null) => {
  if (!rule) return ''
  return locale.value === 'en' ? rule.nameEn || rule.name : rule.name
}

const getRuleDescription = (rule?: SimRule | null) => {
  if (!rule) return ''
  return locale.value === 'en'
    ? rule.descriptionEn || rule.descriptionZh || ''
    : rule.descriptionZh || ''
}

const detailItems = computed(() => {
  const baseItems = [
    {
      label: t('app.simulation.rules.fields.name'),
      value: getRuleName(props.rule),
    },
    {
      label: t('app.simulation.rules.fields.defaultPort'),
      value: props.rule?.defaultPort || t('app.simulation.rules.autoAllocate'),
    },
    {
      label: t('app.simulation.rules.fields.description'),
      value: getRuleDescription(props.rule) || t('common.none'),
    },
  ]

  if (isRedisRule.value) {
    return [
      ...baseItems,
      {
        label: 'Redis AUTH',
        value: requireAuth.value ? 'enabled' : 'disabled',
      },
    ]
  }

  if (isMailRule.value) {
    return [
      ...baseItems,
      {
        label: 'Auth',
        value: requireAuth.value ? 'enabled' : 'disabled',
      },
      {
        label: 'Banner',
        value: banner.value || t('common.none'),
      },
    ]
  }

  if (isCredsRule.value) {
    const items = [...baseItems]
    if (banner.value) {
      items.push({
        label: 'Banner',
        value: banner.value,
      })
    }
    const serverName =
      typeof parsedConfig.value?.server_name === 'string' ? parsedConfig.value.server_name : ''
    if (serverName) {
      items.push({
        label: 'Server Name',
        value: serverName,
      })
    }
    const allowAnonymous = parsedConfig.value?.allow_anonymous
    if (allowAnonymous !== undefined) {
      items.push({
        label: 'Allow Anonymous',
        value: allowAnonymous ? 'Yes' : 'No',
      })
    }
    return items
  }

  return [
    ...baseItems,
    {
      label: 'Server Header',
      value: serverHeader.value || t('app.simulation.rules.systemDefaultNginx'),
    },
  ]
})

const handleLaunchBrowser = () => {
  emit('launchBrowser', parsedConfig.value?.html as string | undefined)
}
</script>

<template>
  <SecLabDialog
    :visible="visible"
    :title="t('app.simulation.rules.detailAndInteractions')"
    width="800px"
    @close="emit('close')"
  >
    <div class="dialog-detail-content flex-column gap-layout">
      <div class="section-card">
        <h4>{{ t('app.simulation.rules.basicParams') }}</h4>
        <SecLabDescriptions :items="detailItems" :column="2" border />
      </div>

      <RedisRuleDetail v-if="isRedisRule" :config="parsedConfig" />
      <MailRuleDetail v-else-if="isMailRule" :protocol="protocol" :config="parsedConfig" />
      <CredsRuleDetail v-else-if="isCredsRule" :protocol="protocol" :config="parsedConfig" />
      <HttpRuleDetail v-else :config="parsedConfig" @launch-browser="handleLaunchBrowser" />
    </div>

    <template #footer>
      <SecLabButton type="secondary" @click="emit('close')">
        {{ t('app.simulation.rules.closeDetails') }}
      </SecLabButton>
    </template>
  </SecLabDialog>
</template>

<style scoped>
.flex-column {
  display: flex;
  flex-direction: column;
}

.gap-layout {
  gap: var(--sdl-space-4);
}

.section-card h4 {
  margin: 0 0 12px;
  font-size: 14px;
  color: var(--sdl-primary);
  font-weight: 600;
}
</style>
