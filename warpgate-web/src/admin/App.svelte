<script lang="ts">
    import { setContext } from 'svelte'
    import { get } from 'svelte/store'
    import { serverInfo, reloadServerInfo } from 'gateway/lib/store'

    import Router, { link, location as routeLocation, push, type RouteDetail, type WrappedComponent } from 'svelte-spa-router'
    import active from 'svelte-spa-router/active'
    import { wrap } from 'svelte-spa-router/wrap'
    import ThemeSwitcher from 'common/ThemeSwitcher.svelte'
    import AuthBar from 'common/AuthBar.svelte'
    import Brand from 'common/Brand.svelte'
    import Loadable from 'common/Loadable.svelte'

    setContext('warpgate.profileHref', '/@warpgate#/gateway/profile')

    async function init () {
        await reloadServerInfo()
    }

    const initPromise = init()
    let fullScreenRoute = $derived($routeLocation.startsWith('/gateway/web-ssh/'))

    async function requireLogin (detail: RouteDetail) {
        await initPromise
        if (!get(serverInfo)?.username) {
            let url = location.pathname + '#' + detail.location
            if (detail.querystring) {
                url += '?' + detail.querystring
            }
            push('/gateway/login?next=' + encodeURIComponent(url))
            return false
        }
        return true
    }

    const routes: Record<string, WrappedComponent> = {
        '/': wrap({
            asyncComponent: () => import('./Home.svelte') as any,
            conditions: [requireLogin],
        }),
        '/gateway/web-ssh/:sessionId': wrap({
            asyncComponent: () => import('../gateway/WebSsh.svelte') as any,
            conditions: [requireLogin],
        }),
        '/profile': wrap({
            asyncComponent: () => import('common/RouteRedirect.svelte') as any,
            props: {
                to: '/gateway/profile',
            },
        }),
        '/sessions/:id': wrap({
            asyncComponent: () => import('./Session.svelte') as any,
            conditions: [requireLogin],
        }),
        '/recordings/:id': wrap({
            asyncComponent: () => import('./Recording.svelte') as any,
            conditions: [requireLogin],
        }),
        '/log': wrap({
            asyncComponent: () => import('./Log.svelte') as any,
            conditions: [requireLogin],
        }),
        '/log/user/:id': wrap({
            asyncComponent: () => import('./Log.svelte') as any,
            props: {
                filterKind: 'user',
            },
            conditions: [requireLogin],
        }),
        '/log/access-role/:id': wrap({
            asyncComponent: () => import('./Log.svelte') as any,
            props: {
                filterKind: 'access-role',
            },
            conditions: [requireLogin],
        }),
        '/log/admin-role/:id': wrap({
            asyncComponent: () => import('./Log.svelte') as any,
            props: {
                filterKind: 'admin-role',
            },
            conditions: [requireLogin],
        }),
        '/config': wrap({
            asyncComponent: () => import('./config/Config.svelte') as any,
            conditions: [requireLogin],
        }),
        '/gateway': wrap({
            asyncComponent: () => import('../gateway/App.svelte') as any,
            props: {
                routePrefix: '/gateway',
                hideAdminButton: true,
                brandHref: '/@warpgate#/gateway',
                embedded: true,
            },
        }),
    }
    routes['/config/*'] = routes['/config']!
    routes['/gateway/*'] = routes['/gateway']!
</script>

<Loadable promise={initPromise}>
    <div class="app {fullScreenRoute ? 'fullscreen' : 'container-lg'}">
        {#if !fullScreenRoute}
        <header>
            <a href="/@warpgate" class="d-flex logo-link me-4">
                <Brand />
            </a>
            {#if $serverInfo?.username}
                <nav class="main-nav" aria-label="Primary">
                    <a class="main-nav-link" use:link use:active href="/">Sessions</a>
                    <a class="main-nav-link" use:link use:active href="/gateway">Gateway</a>
                    <a class="main-nav-link" use:link use:active href="/config">Config</a>
                    <a class="main-nav-link" use:link use:active href="/log">Log</a>
                </nav>
            {/if}
            <div class="ms-auto">
                <AuthBar />
            </div>
        </header>
        {/if}
        <main>
            <Router {routes}/>
        </main>

        {#if !fullScreenRoute}
        <footer>
            <span class="me-auto ms-3">
                {$serverInfo?.version}
            </span>
            <ThemeSwitcher />
        </footer>
        {/if}
    </div>
</Loadable>

<style lang="scss">
    @media (max-width: 767px) {
        .logo-link {
            display: none !important;
        }
    }

    .app {
        min-height: 100vh;
        display: flex;
        flex-direction: column;
    }

    .app.fullscreen {
        max-width: none;
        padding: 0;
    }

    header, footer {
        flex: none;
    }

    main {
        flex: 1 0 0;
    }

    header {
        display: flex;
        align-items: center;
        gap: .2rem;
        min-height: 52px;
        padding: 0;
        margin: 0 0 1rem;
        overflow-x: auto;

        .main-nav {
            display: flex;
            align-items: center;
            gap: .2rem;
        }

        .main-nav-link {
            color: var(--bs-body-color);
            font-size: .95rem;
            font-weight: 500;
            line-height: 1;
            margin-right: 0;
            padding: .45rem .65rem;
            text-decoration: none;
            white-space: nowrap;
            border-radius: var(--wg-option-radius);
            transition:
                background-color .12s ease-out,
                color .12s ease-out,
                box-shadow .12s ease-out,
                transform .08s ease-out;
        }

        .main-nav-link:hover,
        .main-nav-link:focus-visible {
            background: var(--wg-option-hover-bg);
            color: var(--wg-option-hover-color);
        }

        .main-nav-link:focus-visible {
            box-shadow: var(--wg-option-focus-ring);
            outline: none;
        }

        .main-nav-link:active {
            transform: translateY(1px);
        }

        .main-nav-link:global(.active) {
            background: var(--wg-option-active-bg);
            color: var(--wg-option-active-color);
            box-shadow: var(--wg-option-active-indicator);
        }

        .logo-link {
            flex: 0 0 auto;
            margin-right: 1rem !important;
        }
    }

    @media (max-width: 576px) {
        header {
            gap: .5rem;
        }

        header .main-nav {
            flex: 1 0 auto;
        }
    }

    .app:not(.fullscreen) main {
        padding-top: .25rem;
    }
</style>
