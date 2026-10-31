import type { Meta, StoryObj } from '@storybook/react-vite'
import { fn } from 'storybook/test'
import { DownloadJobRow } from '.'

const meta = {
  title: 'download-jobs/DownloadJobRow',
  component: DownloadJobRow,
  parameters: {
    layout: 'centered',
  },
  tags: [],
  argTypes: {
    onCancel: { action: 'onCancel' },
    onRetry: { action: 'onRetry' },
    onDismiss: { action: 'onDismiss' },
  },
  args: {
    job: {
      id: 'job-1',
      assetName: 'Sample Avatar',
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
    },
    onCancel: fn(),
    onRetry: fn(),
    onDismiss: fn(),
  },
  decorators: [
    (Story) => (
      <div className="w-[420px] rounded-md border">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof DownloadJobRow>

export default meta
type Story = StoryObj<typeof meta>

export const Queued: Story = {}

export const DownloadingWithSize: Story = {
  args: {
    job: {
      ...meta.args.job,
      status: 'Downloading',
      currentFile: 'Textures.zip',
      fileIndex: 2,
      fileCount: 3,
      downloadedBytes: 52_428_800,
      totalBytes: 209_715_200,
    },
  },
}

export const DownloadingUnknownSize: Story = {
  args: {
    job: {
      ...meta.args.job,
      status: 'Downloading',
      currentFile: 'Textures.zip',
      fileIndex: 1,
      fileCount: null,
      downloadedBytes: 52_428_800,
      totalBytes: null,
    },
  },
}

export const Importing: Story = {
  args: {
    job: {
      ...meta.args.job,
      status: 'Importing',
      site: 'GoogleDrive',
      fileIndex: 3,
      fileCount: 3,
    },
  },
}

export const FailedPasswordRequired: Story = {
  args: {
    job: {
      ...meta.args.job,
      status: 'Failed',
      error: 'The archive is encrypted',
      errorKind: 'PasswordRequired',
      failedLine: 1,
    },
  },
}

export const Completed: Story = {
  args: {
    job: {
      ...meta.args.job,
      status: 'Completed',
      site: 'Pixeldrain',
    },
  },
}
