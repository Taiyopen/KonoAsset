import { describe, expect, it } from 'vitest'
import {
  convertToBoothURL,
  extractBoothItemId,
  isBoothURL,
  isGumroadURL,
} from './utils'

describe.concurrent('utils', () => {
  it('executes isBoothURL correctly', () => {
    // 正常
    expect(isBoothURL('https://booth.pm/ja/items/123')).toBe(true)
    expect(isBoothURL('https://booth.pm/ja/items/987654')).toBe(true)
    expect(isBoothURL('https://booth.pm/en/items/123')).toBe(true)
    expect(isBoothURL('https://booth.pm/en/items/987654')).toBe(true)
    expect(isBoothURL('https://example.booth.pm/items/123')).toBe(true)
    expect(isBoothURL('https://example-shop-name.booth.pm/items/1000')).toBe(
      true,
    )
    // クエリパラメータ付き
    expect(isBoothURL('https://booth.pm/ja/items/123?sort=new')).toBe(true)
    expect(
      isBoothURL(
        'https://booth.pm/ja/items/123?utm_source=twitter&utm_medium=social',
      ),
    ).toBe(true)
    expect(isBoothURL('https://example.booth.pm/items/123?ref=kono')).toBe(true)

    // 異常
    expect(isBoothURL('https://booth.pm/ja/items/abcdefg')).toBe(false)
    expect(isBoothURL('https://example.booth.pm/items/abcdefg')).toBe(false)
    expect(isBoothURL('https://example.com/ja/items/123')).toBe(false)
    expect(isBoothURL('https://other.example.com/items/123')).toBe(false)
    expect(isBoothURL('https://booth.pm/long-language-code/items/123')).toBe(
      false,
    )
  })

  it('executes extractBoothItemId correctly', () => {
    // 正常
    expect(extractBoothItemId('https://booth.pm/ja/items/123')).toEqual({
      status: 'ok',
      data: 123,
    })
    expect(extractBoothItemId('https://example.booth.pm/items/123')).toEqual({
      status: 'ok',
      data: 123,
    })
    // クエリパラメータ付き
    expect(
      extractBoothItemId('https://booth.pm/ja/items/456?sort=new'),
    ).toEqual({
      status: 'ok',
      data: 456,
    })
    expect(
      extractBoothItemId(
        'https://example.booth.pm/items/789?utm_source=twitter&utm_medium=social',
      ),
    ).toEqual({
      status: 'ok',
      data: 789,
    })

    // 異常
    expect(extractBoothItemId('https://booth.pm/ja/items/abcdefg')).toEqual({
      status: 'error',
      error: 'Invalid Booth URL specified',
    })
    expect(extractBoothItemId('https://example.com/ja/items/123')).toEqual({
      status: 'error',
      error: 'Invalid Booth URL specified',
    })
  })

  it('executes convertToBoothURL correctly', () => {
    // 正常
    expect(convertToBoothURL(123)).toBe('https://booth.pm/ja/items/123')
    expect(convertToBoothURL(987654)).toBe('https://booth.pm/ja/items/987654')
    expect(convertToBoothURL(1000)).toBe('https://booth.pm/ja/items/1000')
  })

  it('executes isGumroadURL correctly', () => {
    // 正常
    expect(isGumroadURL('https://bananabread.gumroad.com/l/ajfrt')).toBe(true)
    expect(isGumroadURL('https://gumroad.com/l/ajfrt')).toBe(true)
    expect(isGumroadURL('https://bananabread.gumroad.com/l/ajfrt/')).toBe(true)
    expect(
      isGumroadURL('https://bananabread.gumroad.com/l/ajfrt?layout=profile'),
    ).toBe(true)
    expect(isGumroadURL('https://seller-name.gumroad.com/l/My_Item-2')).toBe(
      true,
    )

    // 異常
    expect(isGumroadURL('http://bananabread.gumroad.com/l/ajfrt')).toBe(false)
    expect(isGumroadURL('https://bananabread.gumroad.com/')).toBe(false)
    expect(isGumroadURL('https://bananabread.gumroad.com/l/')).toBe(false)
    expect(isGumroadURL('https://evilgumroad.com/l/ajfrt')).toBe(false)
    expect(isGumroadURL('https://gumroad.com.evil.com/l/ajfrt')).toBe(false)
    expect(isGumroadURL('https://booth.pm/ja/items/6641548')).toBe(false)
  })
})
