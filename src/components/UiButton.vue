<template>
  <button
    :type="type"
    class="classic-btn"
    :class="btnClass"
    :disabled="disabled || loading"
    :aria-busy="loading || undefined"
    :aria-pressed="toggle ? active : undefined"
    :title="unavailable ? 'Unavailable' : undefined"
  >
    <Loader2 v-if="loading" :size="iconSize" class="animate-spin shrink-0" />
    <slot />
  </button>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { Loader2 } from '@lucide/vue'

const props = withDefaults(
  defineProps<{
    variant?: 'secondary' | 'primary' | 'ghost' | 'danger' | 'tile'
    size?: 'sm' | 'md' | 'lg'
    /** Toggle / selected state (maps to classic-btn-on) */
    active?: boolean
    /** When true, exposes aria-pressed from active */
    toggle?: boolean
    /** Entity exists but HA state is unavailable/unknown — visual only, still clickable */
    unavailable?: boolean
    disabled?: boolean
    loading?: boolean
    type?: 'button' | 'submit' | 'reset'
  }>(),
  {
    variant: 'secondary',
    size: 'md',
    active: false,
    toggle: false,
    unavailable: false,
    disabled: false,
    loading: false,
    type: 'button',
  }
)

const btnClass = computed(() => {
  const classes: string[] = []
  if (props.variant === 'primary') classes.push('classic-btn-primary')
  else if (props.variant === 'ghost') classes.push('classic-btn-ghost')
  else if (props.variant === 'danger') classes.push('classic-btn-danger')
  else if (props.variant === 'tile') classes.push('classic-btn-tile')

  if (props.size === 'sm') classes.push('classic-btn-sm')
  else if (props.size === 'lg') classes.push('classic-btn-lg')

  if (props.active && !props.unavailable) classes.push('classic-btn-on')
  if (props.unavailable) classes.push('classic-btn-unavailable')
  return classes
})

const iconSize = computed(() => (props.size === 'lg' ? 14 : props.size === 'sm' ? 10 : 12))
</script>
