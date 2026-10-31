import { describe, expect, test } from 'vitest'
import { DownloadJob } from '@/lib/bindings'
import {
  activeCount,
  detectTransitions,
  errorKindLocaleKey,
  isCancelRejectedWhileImporting,
  progressPercent,
  removeJob,
  upsertJob,
} from './logic'

const job = (overrides: Partial<DownloadJob> = {}): DownloadJob => ({
  id: 'job-1',
  assetName: 'Asset',
  status: 'Queued',
  site: 'Workupload',
  currentFile: null,
  fileIndex: 0,
  fileCount: null,
  downloadedBytes: 0,
  totalBytes: null,
  error: null,
  errorKind: null,
  failedLine: null,
  createdAt: 0,
  ...overrides,
})

describe('upsertJob', () => {
  test('replaces an existing job at the same position', () => {
    const jobs = [
      job({ id: 'a' }),
      job({ id: 'b', status: 'Queued' }),
      job({ id: 'c' }),
    ]

    const result = upsertJob(jobs, job({ id: 'b', status: 'Downloading' }))

    expect(result.map((j) => j.id)).toEqual(['a', 'b', 'c'])
    expect(result[1].status).toBe('Downloading')
  })

  test('prepends a new job', () => {
    const jobs = [job({ id: 'a' }), job({ id: 'b' })]

    const result = upsertJob(jobs, job({ id: 'new' }))

    expect(result.map((j) => j.id)).toEqual(['new', 'a', 'b'])
  })

  test('does not mutate the input', () => {
    const jobs = [job({ id: 'a' })]

    upsertJob(jobs, job({ id: 'new' }))

    expect(jobs).toHaveLength(1)
  })
})

describe('removeJob', () => {
  test('removes the job with the given id', () => {
    const jobs = [job({ id: 'a' }), job({ id: 'b' }), job({ id: 'c' })]

    expect(removeJob(jobs, 'b').map((j) => j.id)).toEqual(['a', 'c'])
  })

  test('returns the same contents when the id is unknown', () => {
    const jobs = [job({ id: 'a' })]

    expect(removeJob(jobs, 'x')).toEqual(jobs)
  })
})

describe('detectTransitions', () => {
  test('fires only when the status becomes Completed or Failed', () => {
    expect(
      detectTransitions(
        job({ status: 'Importing' }),
        job({ status: 'Completed' }),
      ),
    ).toBe('completed')
    expect(
      detectTransitions(
        job({ status: 'Completed' }),
        job({ status: 'Completed' }),
      ),
    ).toBeNull()
    expect(detectTransitions(undefined, job({ status: 'Failed' }))).toBe(
      'failed',
    )
    expect(
      detectTransitions(
        job({ status: 'Queued' }),
        job({ status: 'Downloading' }),
      ),
    ).toBeNull()
  })

  test('does not fire for a job that is first seen in a non-final state', () => {
    expect(detectTransitions(undefined, job({ status: 'Downloading' }))).toBe(
      null,
    )
  })

  test('fires again after a failed job is retried and fails again', () => {
    expect(
      detectTransitions(job({ status: 'Queued' }), job({ status: 'Failed' })),
    ).toBe('failed')
  })
})

describe('progressPercent', () => {
  test('returns a percentage when the total is known', () => {
    expect(progressPercent(job({ downloadedBytes: 50, totalBytes: 200 }))).toBe(
      25,
    )
  })

  test('returns null when the total is unknown or zero', () => {
    expect(
      progressPercent(job({ downloadedBytes: 50, totalBytes: null })),
    ).toBeNull()
    expect(
      progressPercent(job({ downloadedBytes: 0, totalBytes: 0 })),
    ).toBeNull()
  })

  test('is clamped to 0-100', () => {
    expect(
      progressPercent(job({ downloadedBytes: 300, totalBytes: 200 })),
    ).toBe(100)
  })
})

describe('activeCount', () => {
  test('counts Queued, Downloading and Importing only', () => {
    const jobs = [
      job({ id: '1', status: 'Queued' }),
      job({ id: '2', status: 'Downloading' }),
      job({ id: '3', status: 'Importing' }),
      job({ id: '4', status: 'Completed' }),
      job({ id: '5', status: 'Failed' }),
      job({ id: '6', status: 'Cancelled' }),
    ]

    expect(activeCount(jobs)).toBe(3)
  })
})

describe('errorKindLocaleKey', () => {
  test.each([
    ['PasswordRequired', 'download-jobs:error:password-required'],
    ['WrongPassword', 'download-jobs:error:wrong-password'],
    ['NotFound', 'download-jobs:error:not-found'],
    ['LoginRequired', 'download-jobs:error:login-required'],
    ['RateLimited', 'download-jobs:error:rate-limited'],
    ['NotAFile', 'download-jobs:error:not-a-file'],
    ['ImportFailed', 'download-jobs:error:import-failed'],
    ['Other', 'download-jobs:error:other'],
  ] as const)('%s -> %s', (kind, key) => {
    expect(errorKindLocaleKey(kind)).toBe(key)
  })

  test('falls back to other when the kind is missing', () => {
    expect(errorKindLocaleKey(null)).toBe('download-jobs:error:other')
  })
})

describe('isCancelRejectedWhileImporting', () => {
  test('matches only the backend refusal for importing jobs', () => {
    expect(
      isCancelRejectedWhileImporting(
        'Download job 1 cannot be cancelled while importing',
      ),
    ).toBe(true)
    expect(isCancelRejectedWhileImporting('Download job 1 not found')).toBe(
      false,
    )
  })
})
