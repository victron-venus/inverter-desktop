<template>
  <section ref="container" class="tariff-sheet" aria-label="Weekly electricity rates" />
</template>
<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue'
import { createUniver, LocaleType } from '@univerjs/presets'
import { UniverSheetsCorePreset } from '@univerjs/preset-sheets-core'
import enUS from '@univerjs/preset-sheets-core/locales/en-US'
import '@univerjs/preset-sheets-core/lib/index.css'
import { DAYS, SLOTS, slotLabel, type RateGrid } from './model'

const props = defineProps<{ rates: RateGrid }>()
const container = ref<HTMLElement | null>(null)
let runtime: ReturnType<typeof createUniver> | undefined
const emit = defineEmits<{ error: [message: string] }>()

onMounted(() => {
  if (!container.value) return
  try {
    runtime = createUniver({
      locale: LocaleType.EN_US,
      locales: { [LocaleType.EN_US]: enUS },
      presets: [
        UniverSheetsCorePreset({
          container: container.value,
          header: false,
          toolbar: false,
          formulaBar: false,
          footer: false,
          contextMenu: false,
        }),
      ],
    })
    const workbook = runtime.univerAPI.createWorkbook({
      id: 'electricity-tariff',
      name: 'Weekly rates',
      sheetOrder: ['week'],
      sheets: {
        week: {
          id: 'week',
          name: 'Weekly rates',
          rowCount: SLOTS + 1,
          columnCount: 8,
          defaultColumnWidth: 105,
          defaultRowHeight: 28,
          columnData: { 0: { w: 150 } },
          freeze: { startRow: 1, startColumn: 1, xSplit: 1, ySplit: 1 },
        },
      },
    })
    workbook
      .getActiveSheet()
      .getRange(0, 0, SLOTS + 1, 8)
      .setValues([
        ['Local time', ...DAYS],
        ...props.rates.map((row, slot) => [slotLabel(slot), ...row.map((rate) => rate ?? '')]),
      ])
  } catch {
    emit('error', 'The spreadsheet could not be opened. Close the editor and try again.')
  }
})
onBeforeUnmount(() => runtime?.univer.dispose())

async function getRates(): Promise<RateGrid> {
  const workbook = runtime?.univerAPI.getActiveWorkbook()
  if (!workbook) throw new Error('The spreadsheet is still loading.')
  await workbook.endEditingAsync(true)
  const snapshot = workbook.save()
  if (snapshot.sheetOrder.length !== 1 || snapshot.sheetOrder[0] !== 'week')
    throw new Error('Keep the single weekly tariff sheet.')
  const data = snapshot.sheets.week.cellData ?? {}
  const headers = ['Local time', ...DAYS]
  if (headers.some((label, column) => data[0]?.[column]?.v !== label))
    throw new Error('Keep the original day headings and edit only the rate cells.')
  return Array.from({ length: SLOTS }, (_, slot) => {
    if (data[slot + 1]?.[0]?.v !== slotLabel(slot))
      throw new Error('Keep the original time labels and edit only the rate cells.')
    return DAYS.map((_, day) => {
      const cell = data[slot + 1]?.[day + 1]
      if (cell?.f || cell?.p)
        throw new Error(
          'Enter numeric prices directly; formulas and rich text are not tariff rates.'
        )
      if (cell?.v === undefined || cell.v === null || cell.v === '') return null
      if (typeof cell.v !== 'number')
        throw new Error(`Enter a numeric price for ${DAYS[day]}, ${slotLabel(slot)}.`)
      return cell.v
    })
  })
}
defineExpose({ getRates })
</script>
<style scoped>
.tariff-sheet {
  height: 440px;
  min-height: 300px;
  width: 100%;
  color: #20252b;
  background: white;
}
</style>
