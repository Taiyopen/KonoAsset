import { beforeEach, describe, expect, test, vi } from 'vitest'
import type { DownloadJob } from '@/lib/bindings'

type Listener = (e: { payload: DownloadJob }) => void

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  listDownloadJobs: vi.fn(),
}))

vi.mock('@/lib/bindings', () => ({
  commands: { listDownloadJobs: mocks.listDownloadJobs },
  events: { downloadJobUpdated: { listen: mocks.listen } },
}))

import { useDownloadJobStore } from '.'

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

let listener: Listener | undefined

const emit = (j: DownloadJob) => {
  if (listener === undefined) throw new Error('no listener registered')
  listener({ payload: j })
}

beforeEach(() => {
  listener = undefined
  mocks.listen.mockReset()
  mocks.listDownloadJobs.mockReset()
  mocks.listen.mockImplementation(async (fn: Listener) => {
    listener = fn
    return () => {}
  })
  mocks.listDownloadJobs.mockResolvedValue({ status: 'ok', data: [] })
  useDownloadJobStore.setState({ jobs: [], initialized: false })
})

describe('initialize', () => {
  test('registers one listener and lists once when called twice', async () => {
    const onTransition = vi.fn()

    await Promise.all([
      useDownloadJobStore.getState().initialize(onTransition),
      useDownloadJobStore.getState().initialize(onTransition),
    ])
    await useDownloadJobStore.getState().initialize(onTransition)

    expect(mocks.listen).toHaveBeenCalledTimes(1)
    expect(mocks.listDownloadJobs).toHaveBeenCalledTimes(1)
  })

  test('populates jobs from the list without firing transitions', async () => {
    const onTransition = vi.fn()
    mocks.listDownloadJobs.mockResolvedValue({
      status: 'ok',
      data: [job({ id: 'a', status: 'Completed' }), job({ id: 'b' })],
    })

    await useDownloadJobStore.getState().initialize(onTransition)

    expect(useDownloadJobStore.getState().jobs.map((j) => j.id)).toEqual([
      'a',
      'b',
    ])
    expect(onTransition).not.toHaveBeenCalled()
  })

  test('an event delivered while the list is pending is not overwritten by the older list entry', async () => {
    let resolveList: (value: unknown) => void = () => {}
    mocks.listDownloadJobs.mockReturnValue(
      new Promise((resolve) => {
        resolveList = resolve
      }),
    )

    const initializing = useDownloadJobStore.getState().initialize(vi.fn())
    await vi.waitFor(() => expect(listener).toBeDefined())

    emit(job({ id: 'a', status: 'Downloading' }))
    resolveList({
      status: 'ok',
      data: [job({ id: 'a', status: 'Queued' }), job({ id: 'b' })],
    })
    await initializing

    const jobs = useDownloadJobStore.getState().jobs
    expect(jobs.find((j) => j.id === 'a')?.status).toBe('Downloading')
    expect(jobs.map((j) => j.id).sort()).toEqual(['a', 'b'])
  })

  test('a Completed event fires onTransition once and a duplicate does not', async () => {
    const onTransition = vi.fn()
    await useDownloadJobStore.getState().initialize(onTransition)

    emit(job({ status: 'Importing' }))
    emit(job({ status: 'Completed' }))
    emit(job({ status: 'Completed' }))

    expect(onTransition).toHaveBeenCalledTimes(1)
    expect(onTransition).toHaveBeenCalledWith(
      expect.objectContaining({ status: 'Completed' }),
      'completed',
    )
  })

  test('retries on a later call when listen throws', async () => {
    mocks.listen.mockRejectedValueOnce(new Error('boom'))
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    await useDownloadJobStore.getState().initialize(vi.fn())
    expect(useDownloadJobStore.getState().initialized).toBe(false)

    await useDownloadJobStore.getState().initialize(vi.fn())
    expect(useDownloadJobStore.getState().initialized).toBe(true)
    expect(mocks.listen).toHaveBeenCalledTimes(2)

    errorSpy.mockRestore()
  })

  test('does not register a second listener when only the list call fails', async () => {
    mocks.listDownloadJobs.mockRejectedValueOnce(new Error('boom'))
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    await useDownloadJobStore.getState().initialize(vi.fn())
    await useDownloadJobStore.getState().initialize(vi.fn())

    expect(mocks.listen).toHaveBeenCalledTimes(1)

    errorSpy.mockRestore()
  })
})

describe('removeJob', () => {
  test('a dismissed job stays removed when other jobs update', async () => {
    mocks.listDownloadJobs.mockResolvedValue({
      status: 'ok',
      data: [job({ id: 'a', status: 'Completed' }), job({ id: 'b' })],
    })
    await useDownloadJobStore.getState().initialize(vi.fn())

    useDownloadJobStore.getState().removeJob('a')
    emit(job({ id: 'b', status: 'Downloading' }))

    expect(useDownloadJobStore.getState().jobs.map((j) => j.id)).toEqual(['b'])
  })
})
