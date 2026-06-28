import { ref, watch, type ComputedRef } from 'vue'

type KeyValueRow = { key: string; value: string }
type ListFieldKey = 'env' | 'ports' | 'volumes'

type ContainerForm = {
  name: string
  command: string
  env: string
  ports: string
  volumes: string
  restartPolicy: string
  network: string
  autoRemove: boolean
}

type UseContainerCreateFormOptions = {
  containerForm: ComputedRef<ContainerForm | undefined>
  selectedImageId: ComputedRef<string | null | undefined>
  isCreateActive: ComputedRef<boolean | undefined>
  t: (key: string) => string
  onUpdateContainerForm: (value: ContainerForm) => void
  onUpdateSelectedImageId: (value: string | null) => void
  onUpdateStep: (value: 'selectImage' | 'config') => void
  onCancelCreate: () => void
  onSubmitCreate: () => void
  onCreatePanelOpened: () => void
  onError: (message: string) => void
}

export const useContainerCreateForm = ({
  containerForm,
  selectedImageId,
  isCreateActive,
  t,
  onUpdateContainerForm,
  onUpdateSelectedImageId,
  onUpdateStep,
  onCancelCreate,
  onSubmitCreate,
  onCreatePanelOpened,
  onError,
}: UseContainerCreateFormOptions) => {
  const envRows = ref<KeyValueRow[]>([{ key: '', value: '' }])
  const portRows = ref<KeyValueRow[]>([{ key: '', value: '' }])
  const volumeRows = ref<KeyValueRow[]>([{ key: '', value: '' }])

  const normalizeTextModelValue = (value: string | number | boolean | undefined | null): string => {
    if (typeof value === 'string') return value
    if (value === undefined || value === null) return ''
    return String(value)
  }

  const parseKeyValueLines = (input: string | undefined, delimiter: string) => {
    const lines =
      input
        ?.split('\n')
        .map((line) => line.trim())
        .filter((line) => line.length > 0) || []
    if (lines.length === 0) {
      return [{ key: '', value: '' }]
    }
    return lines.map((line) => {
      const [left, right] = line.split(delimiter)
      return {
        key: (left || '').trim(),
        value: (right || '').trim(),
      }
    })
  }

  const syncRowsFromForm = () => {
    const form = containerForm.value
    if (!form) return
    envRows.value = parseKeyValueLines(form.env, '=')
    portRows.value = parseKeyValueLines(form.ports, ':')
    volumeRows.value = parseKeyValueLines(form.volumes, ':')
  }

  const updateFormField = (key: keyof ContainerForm, value: string | boolean) => {
    const form = containerForm.value
    if (!form) return
    onUpdateContainerForm({
      ...form,
      [key]: value,
    })
  }

  const updateFormListField = (key: ListFieldKey, rows: KeyValueRow[]) => {
    const delimiter = key === 'env' ? '=' : ':'
    const value = rows
      .map((row) => `${row.key.trim()}${delimiter}${row.value.trim()}`)
      .filter((row) => row !== delimiter)
      .join('\n')
    updateFormField(key, value)
  }

  const updateListRow = (
    key: ListFieldKey,
    index: number,
    field: 'key' | 'value',
    value: string,
  ) => {
    const target = key === 'env' ? envRows : key === 'ports' ? portRows : volumeRows
    const next = target.value.map((row, idx) => (idx === index ? { ...row, [field]: value } : row))
    target.value = next
    updateFormListField(key, next)
  }

  const addListRow = (key: ListFieldKey) => {
    const target = key === 'env' ? envRows : key === 'ports' ? portRows : volumeRows
    target.value = [...target.value, { key: '', value: '' }]
  }

  const removeListRow = (key: ListFieldKey, index: number) => {
    const target = key === 'env' ? envRows : key === 'ports' ? portRows : volumeRows
    if (target.value.length === 1) {
      target.value = [{ key: '', value: '' }]
    } else {
      target.value = target.value.filter((_, idx) => idx !== index)
    }
    updateFormListField(key, target.value)
  }

  const handleListRowModelUpdate = (
    key: ListFieldKey,
    index: number,
    field: 'key' | 'value',
    value: string | number | boolean | undefined | null,
  ) => {
    updateListRow(key, index, field, normalizeTextModelValue(value))
  }

  const handleSelectedImageIdChange = (value: string | number | boolean | undefined | null) => {
    const next = normalizeTextModelValue(value)
    onUpdateSelectedImageId(next || null)
  }

  const handleContainerNameChange = (value: string | number | boolean | undefined | null) => {
    updateFormField('name', normalizeTextModelValue(value))
  }

  const handleContainerCommandChange = (value: string | number | boolean | undefined | null) => {
    updateFormField('command', normalizeTextModelValue(value))
  }

  const handleRestartPolicyChange = (value: string | number | boolean | undefined | null) => {
    updateFormField('restartPolicy', normalizeTextModelValue(value))
  }

  const handleNetworkChange = (value: string | number | boolean | undefined | null) => {
    updateFormField('network', normalizeTextModelValue(value))
  }

  const handleAutoRemoveChange = (value: string | number | boolean | undefined | null) => {
    updateFormField('autoRemove', Boolean(value))
  }

  const getListActionLabel = (rows: KeyValueRow[]) =>
    rows.length === 1 ? t('app.docker.containers.clear') : t('app.docker.containers.remove')

  const proceedCreate = () => {
    if (!selectedImageId.value) {
      onError('请选择一个镜像以继续')
      return
    }
    onUpdateStep('config')
  }

  const backToSelect = () => {
    onUpdateStep('selectImage')
  }

  const cancelCreate = () => {
    onCancelCreate()
  }

  const submitCreate = () => {
    onSubmitCreate()
  }

  watch(
    isCreateActive,
    (active) => {
      if (!active) return
      syncRowsFromForm()
      onCreatePanelOpened()
    },
    { immediate: true },
  )

  return {
    envRows,
    portRows,
    volumeRows,
    handleSelectedImageIdChange,
    handleContainerNameChange,
    handleContainerCommandChange,
    handleRestartPolicyChange,
    handleNetworkChange,
    handleAutoRemoveChange,
    handleListRowModelUpdate,
    addListRow,
    removeListRow,
    getListActionLabel,
    proceedCreate,
    backToSelect,
    cancelCreate,
    submitCreate,
  }
}
