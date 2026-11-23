import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  // `next dev` otherwise writes an AGENTS.md here whenever a coding agent runs it; the
  // repository keeps its own agent rules.
  agentRules: false,
  devIndicators: false,
  transpilePackages: ['@koharu/ipc', '@koharu/ui'],
  output: 'export',
  images: {
    unoptimized: true,
  },
}

export default nextConfig
