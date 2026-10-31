import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { DownloadJob } from '@/lib/bindings'
import { DownloadJobRow } from './DownloadJobRow'

const failed = (overrides: Partial<DownloadJob> = {}): DownloadJob => ({
  id: 'job-1',
  assetName: 'Asset',
  status: 'Failed',
  site: 'Workupload',
  currentFile: null,
  fileIndex: 0,
  fileCount: null,
  downloadedBytes: 0,
  totalBytes: null,
  error: 'wrong password',
  errorKind: 'WrongPassword',
  failedLine: 1,
  createdAt: 0,
  ...overrides,
})

const renderRow = (job: DownloadJob) => {
  const onRetry = vi.fn()
  render(
    <DownloadJobRow
      job={job}
      onCancel={vi.fn()}
      onRetry={onRetry}
      onDismiss={vi.fn()}
    />,
  )
  return onRetry
}

describe('DownloadJobRow', () => {
  afterEach(() => {
    cleanup()
  })

  it('needs a password before retrying a password error', async () => {
    const onRetry = renderRow(failed())
    const retry = screen.getByRole<HTMLButtonElement>('button', {
      name: 'Retry',
    })
    expect(retry.disabled).toBe(true)

    await userEvent.type(screen.getByRole('textbox'), 'secret')
    expect(retry.disabled).toBe(false)

    await userEvent.keyboard('{Enter}')
    expect(onRetry).toHaveBeenCalledExactlyOnceWith('job-1', 'secret')
  })

  it('does not retry on Enter with an empty password', async () => {
    const onRetry = renderRow(failed({ errorKind: 'PasswordRequired' }))

    await userEvent.type(screen.getByRole('textbox'), '{Enter}')
    expect(onRetry).not.toHaveBeenCalled()
  })

  it('retries other errors without a password', async () => {
    const onRetry = renderRow(failed({ errorKind: 'NotFound' }))

    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(onRetry).toHaveBeenCalledExactlyOnceWith('job-1', null)
  })
})
