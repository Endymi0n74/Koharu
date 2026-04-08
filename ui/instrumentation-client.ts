import * as Sentry from '@sentry/nextjs'

Sentry.init({
  dsn: 'https://8bdbe0ecdeeccd201da90456692824a3@o4511181517815808.ingest.us.sentry.io/4511181706821632',
  tracesSampleRate: process.env.NODE_ENV === 'development' ? 1 : 0.1,
})

export const onRouterTransitionStart = Sentry.captureRouterTransitionStart
