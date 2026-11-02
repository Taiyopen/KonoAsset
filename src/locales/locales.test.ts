import { describe, expect, it } from 'vitest'
import enUs from '@/locales/en-US.json'
import enGb from '@/locales/en-GB.json'
import jaJp from '@/locales/ja-JP.json'
import zhCn from '@/locales/zh-CN.json'
import zhTw from '@/locales/zh-TW.json'

const locales = { enUs, enGb, jaJp, zhCn, zhTw }

describe('locales', () => {
  // 重複確認は BOOTH と Gumroad のどちらの重複でも表示される
  it('does not name BOOTH in the duplicate warning shared with Gumroad', () => {
    for (const [name, locale] of Object.entries(locales)) {
      const text = locale.data['addasset:duplicate-warning:warning-text']

      expect(text, name).toBeTruthy()
      expect(text, name).not.toMatch(/BOOTH/i)
    }
  })
})
