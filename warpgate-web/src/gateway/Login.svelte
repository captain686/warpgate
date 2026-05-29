<script lang="ts">
    import { getContext } from 'svelte'
    import { get } from 'svelte/store'
    import { querystring, replace } from 'svelte-spa-router'
    import { FormGroup } from '@sveltestrap/sveltestrap'
    import Fa from 'svelte-fa'
    import { faArrowRight } from '@fortawesome/free-solid-svg-icons'
    import { faGoogle, faMicrosoft, faApple } from '@fortawesome/free-brands-svg-icons'

    import { api, ApiAuthState, LoginFailureResponseFromJSON, type SsoProviderDescription, SsoProviderKind, ResponseError } from 'gateway/lib/api'
    import { reloadServerInfo, serverInfo } from 'gateway/lib/store'
    import { stringifyError } from 'common/errors'
    import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'
    import Loadable from 'common/Loadable.svelte'

    let error: string|null = $state(null)
    let username = $state('')
    let password = $state('')
    let otp = $state('')
    let busy = $state(false)
    let otpInput: HTMLInputElement|undefined = $state()
    let authState: ApiAuthState|undefined = $state()
    let ssoProvidersPromise = api.getSsoProviders()
    let showPasswordLogin = $state(false)
    const getRoutePrefix = getContext<() => string>('warpgate.gatewayRoutePrefix') ?? (() => '')

    const urlQuery = get(querystring) ?? location.search
    const nextURL = new URLSearchParams(urlQuery).get('next') ?? undefined
    const serverErrorMessage = new URLSearchParams(urlQuery).get('login_error')
    const initPromise = init()

    async function init () {
        try {
            authState = (await api.getDefaultAuthState()).state
        } catch (err) {
            if (err instanceof ResponseError) {
                if (err.response.status === 404) {
                    authState = ApiAuthState.NotStarted
                }
            } else {
                throw err
            }
        }
        continueWithState()
    }

    function success () {
        if (nextURL) {
            location.assign(nextURL)
        } else {
            replace(`${getRoutePrefix()}/`)
        }
    }

    async function continueWithState () {
        if (authState === ApiAuthState.Success) {
            success()
        }
        if (authState === ApiAuthState.SsoNeeded) {
            const providers = await ssoProvidersPromise
            if (!providers.length) {
                // todo
            }
            if (providers.length === 1) {
                startSSO(providers[0]!)
            }
        }
        if (authState === ApiAuthState.OtpNeeded) {
            setTimeout(() => {
                otpInput?.focus()
            })
        }
    }

    async function login () {
        busy = true
        try {
            await _login()
        } finally {
            busy = false
        }
    }

    async function _login () {
        error = null
        try {
            if (authState === ApiAuthState.OtpNeeded) {
                await api.otpLogin({
                    otpLoginRequest: {
                        otp,
                    },
                })
            } else {
                await api.login({
                    loginRequest: {
                        username,
                        password,
                    },
                })
            }
            await reloadServerInfo()
            success()
        } catch (err) {
            if (err instanceof ResponseError) {
                if (err.response.status === 401) {
                    const failure = LoginFailureResponseFromJSON(await err.response.json())
                    authState = failure.state

                    continueWithState()
                } else {
                    error = await err.response.text()
                }
            } else {
                error = await stringifyError(err)
            }
        }
    }

    async function cancel () {
        await api.cancelDefaultAuth()
        location.reload()
    }

    async function startSSO (provider: SsoProviderDescription) {
        busy = true
        try {
            const p = await api.startSso({ name: provider.name, next: nextURL })
            location.href = p.url
        } catch (err) {
            error = await stringifyError(err)
            busy = false
        }
    }

    function needsPasswordInput (): boolean {
        return (
            authState === ApiAuthState.NotStarted ||
            authState === ApiAuthState.PasswordNeeded ||
            authState === ApiAuthState.Failed ||
            authState === ApiAuthState.IpRejected
        ) && (!$serverInfo?.minimizePasswordLogin || showPasswordLogin)
    }

    function canUseSso (): boolean {
        return (
            authState === ApiAuthState.SsoNeeded ||
            authState === ApiAuthState.NotStarted ||
            authState === ApiAuthState.Failed ||
            authState === ApiAuthState.IpRejected
        )
    }

    function canRevealPasswordLogin (): boolean {
        return (
            authState === ApiAuthState.NotStarted ||
            authState === ApiAuthState.PasswordNeeded ||
            authState === ApiAuthState.Failed ||
            authState === ApiAuthState.IpRejected
        ) && !!$serverInfo?.minimizePasswordLogin && !showPasswordLogin
    }

    function canCancel (): boolean {
        return authState !== ApiAuthState.NotStarted && authState !== ApiAuthState.Failed && authState !== ApiAuthState.IpRejected
    }

    function pageTitle (): string {
        switch (authState) {
            case ApiAuthState.OtpNeeded:
                return 'Verify your identity'
            case ApiAuthState.SsoNeeded:
                return 'Continue with single sign-on'
            default:
                return 'Sign in'
        }
    }

    function pageSubtitle (): string {
        switch (authState) {
            case ApiAuthState.OtpNeeded:
                return 'Enter the one-time password to complete authentication.'
            case ApiAuthState.SsoNeeded:
                return 'Choose an approved identity provider to continue.'
            case ApiAuthState.Failed:
                return 'Authentication failed. Review the account details and try again.'
            case ApiAuthState.IpRejected:
                return 'This sign-in attempt is outside the network policy for this account.'
            default:
                return 'Authenticate to access managed sessions and approved targets.'
        }
    }

    function statusLabel (): string {
        switch (authState) {
            case ApiAuthState.OtpNeeded:
                return 'Verification required'
            case ApiAuthState.SsoNeeded:
                return 'Provider selection'
            case ApiAuthState.Failed:
                return 'Authentication failed'
            case ApiAuthState.IpRejected:
                return 'Access denied'
            case ApiAuthState.PasswordNeeded:
                return 'Credentials required'
            case ApiAuthState.Success:
                return 'Authenticated'
            default:
                return 'Ready'
        }
    }

    function flowLabel (): string {
        if (authState === ApiAuthState.OtpNeeded) {
            return 'One-time password'
        }
        if (authState === ApiAuthState.SsoNeeded) {
            return 'Single sign-on'
        }
        if (needsPasswordInput() && canUseSso()) {
            return 'Password or SSO'
        }
        if (needsPasswordInput()) {
            return 'Username and password'
        }
        if (canUseSso()) {
            return 'Single sign-on'
        }
        return 'Authentication'
    }

    function destinationLabel (): string {
        if (!nextURL) {
            return 'Gateway home'
        }
        try {
            const target = new URL(nextURL, location.origin)
            const summary = `${target.origin === location.origin ? '' : target.origin}${target.pathname}${target.search}${target.hash}`
            return summary || 'Gateway home'
        } catch {
            return nextURL
        }
    }

    function providerCaption (provider: SsoProviderDescription): string {
        switch (provider.kind) {
            case SsoProviderKind.Google:
                return 'Google account'
            case SsoProviderKind.Azure:
                return 'Microsoft account'
            case SsoProviderKind.Apple:
                return 'Apple account'
            default:
                return 'Single sign-on provider'
        }
    }
</script>

{#snippet localLoginForm()}
    <form class="auth-form" autocomplete="on" onsubmit={e => {
        login()
        e.preventDefault()
    }}>
        <FormGroup floating label="Username">
            <!-- svelte-ignore a11y_autofocus -->
            <input
                bind:value={username}
                name="username"
                autocomplete="username"
                disabled={busy}
                class="form-control"
                required
                autofocus />
        </FormGroup>

        <FormGroup floating label="Password">
            <input
                bind:value={password}
                name="password"
                type="password"
                autocomplete="current-password"
                disabled={busy}
                required
                class="form-control" />
        </FormGroup>

        <button
            class="btn btn-primary auth-submit-button d-flex align-items-center justify-content-center"
            type="submit"
            disabled={busy}
        >
            Sign in
            <Fa class="ms-2" fw icon={faArrowRight} />
        </button>
    </form>
{/snippet}

<Loadable promise={initPromise}>
    <div class="auth-shell">
        <section class="auth-panel">
            <div class="auth-panel-header">
                <div class="auth-kicker">Secure access</div>
                <div class="page-summary-bar">
                    <div>
                        <h1>{pageTitle()}</h1>
                        <div class="text-muted">{pageSubtitle()}</div>
                    </div>
                </div>

                <div class="auth-meta-grid">
                    <div class="auth-meta-card">
                        <span class="auth-meta-label">Status</span>
                        <span class="auth-meta-value">{statusLabel()}</span>
                    </div>
                    <div class="auth-meta-card">
                        <span class="auth-meta-label">Flow</span>
                        <span class="auth-meta-value">{flowLabel()}</span>
                    </div>
                    <div class="auth-meta-card">
                        <span class="auth-meta-label">After sign-in</span>
                        <span class="auth-meta-value">{destinationLabel()}</span>
                    </div>
                </div>
            </div>

            <div class="auth-panel-body">
                {#if authState === ApiAuthState.Failed || authState === ApiAuthState.IpRejected || serverErrorMessage || error}
                    <div class="auth-feedback">
                        {#if authState === ApiAuthState.Failed}
                            <Alert color="danger">Incorrect credentials</Alert>
                        {/if}
                        {#if authState === ApiAuthState.IpRejected}
                            <Alert color="danger">Login denied: your IP address is not in the allowed range for this user</Alert>
                        {/if}
                        {#if serverErrorMessage}
                            <Alert color="danger">{serverErrorMessage}</Alert>
                        {/if}
                        {#if error}
                            <Alert color="danger">{error}</Alert>
                        {/if}
                    </div>
                {/if}

                {#if authState === ApiAuthState.OtpNeeded}
                    <section class="auth-section">
                        <div class="auth-section-header">
                            <div>
                                <h2>Verification code</h2>
                                <p>Enter the current one-time password from your authenticator app.</p>
                            </div>
                        </div>

                        <form class="auth-form auth-form-otp" onsubmit={e => {
                            login()
                            e.preventDefault()
                        }}>
                            <FormGroup floating label="One-time password" class="w-100">
                                <!-- svelte-ignore a11y_autofocus -->
                                <input
                                    bind:value={otp}
                                    bind:this={otpInput}
                                    name="otp"
                                    autocomplete="one-time-code"
                                    required
                                    pattern={'[0-9]{6,8}'}
                                    autofocus
                                    inputmode="numeric"
                                    disabled={busy}
                                    class="form-control" />
                            </FormGroup>

                            <button
                                class="btn btn-primary auth-submit-button"
                                type="submit"
                                disabled={busy}
                            >
                                Verify
                                <Fa icon={faArrowRight} />
                            </button>
                        </form>
                    </section>
                {/if}

                {#if needsPasswordInput()}
                    <section class="auth-section">
                        <div class="auth-section-header">
                            <div>
                                <h2>Account sign-in</h2>
                                <p>Use your Warpgate username and password to continue.</p>
                            </div>
                        </div>

                        <!-- eslint-disable-next-line @typescript-eslint/no-confusing-void-expression -->
                        {@render localLoginForm()}
                    </section>
                {/if}

                {#if canUseSso()}
                    <Loadable promise={ssoProvidersPromise}>
                        {#snippet children(ssoProviders)}
                            {#if ssoProviders.length}
                                <section class="auth-section">
                                    <div class="auth-divider">
                                        <span>or continue with</span>
                                    </div>

                                    <div class="auth-section-header">
                                        <div>
                                            <h2>Single sign-on</h2>
                                            <p>Use an approved identity provider for this gateway.</p>
                                        </div>
                                    </div>

                                    <div class="sso-buttons">
                                        {#each ssoProviders as ssoProvider (ssoProvider.name)}
                                            <button
                                                type="button"
                                                class="auth-option-button sso-button"
                                                disabled={busy}
                                                onclick={() => startSSO(ssoProvider)}
                                            >
                                                <span class="sso-button-icon">
                                                    {#if ssoProvider.kind === SsoProviderKind.Google}
                                                        <Fa fw icon={faGoogle} />
                                                    {/if}
                                                    {#if ssoProvider.kind === SsoProviderKind.Azure}
                                                        <Fa fw icon={faMicrosoft} />
                                                    {/if}
                                                    {#if ssoProvider.kind === SsoProviderKind.Apple}
                                                        <Fa fw icon={faApple} />
                                                    {/if}
                                                </span>
                                                <span class="auth-option-button-copy">
                                                    <span class="auth-option-button-title">{ssoProvider.label || ssoProvider.name}</span>
                                                    <span class="auth-option-button-description">{providerCaption(ssoProvider)}</span>
                                                </span>
                                                <span class="auth-option-button-arrow">
                                                    <Fa icon={faArrowRight} />
                                                </span>
                                            </button>
                                        {/each}
                                    </div>
                                </section>
                            {/if}
                        {/snippet}
                    </Loadable>
                {/if}

                {#if canRevealPasswordLogin()}
                    <section class="auth-section">
                        <button
                            type="button"
                            class="auth-option-button password-login-link"
                            disabled={busy}
                            onclick={() => {
                                showPasswordLogin = true
                            }}
                        >
                            <span class="auth-option-button-copy">
                                <span class="auth-option-button-title">Use username and password</span>
                                <span class="auth-option-button-description">Sign in with a local Warpgate account instead</span>
                            </span>
                            <span class="auth-option-button-arrow">
                                <Fa icon={faArrowRight} />
                            </span>
                        </button>
                    </section>
                {/if}

                {#if canCancel()}
                    <div class="auth-footer">
                        <button
                            type="button"
                            class="btn btn-secondary auth-cancel-button"
                            onclick={cancel}
                        >
                            Cancel sign-in
                        </button>
                    </div>
                {/if}
            </div>
        </section>
    </div>
</Loadable>

<style lang="scss">
    .auth-shell {
        margin-top: 1.25rem;
    }

    .auth-panel {
        overflow: hidden;
        border: 1px solid var(--bs-border-color);
        border-radius: .75rem;
        background: var(--bs-body-bg);
    }

    .auth-panel-header {
        padding: 1.5rem;
        border-bottom: 1px solid var(--bs-border-color);
        background: var(--bs-tertiary-bg);
    }

    .auth-kicker {
        margin-bottom: .65rem;
        color: var(--bs-secondary-color);
        font-size: .75rem;
        font-weight: 600;
        letter-spacing: .04rem;
        text-transform: uppercase;
    }

    .auth-panel-header :global(.page-summary-bar) {
        margin: 0;
    }

    .auth-panel-header :global(.text-muted) {
        max-width: 38rem;
        line-height: 1.45;
    }

    .auth-meta-grid {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: .75rem;
        margin-top: 1rem;
    }

    .auth-meta-card {
        display: grid;
        gap: .2rem;
        min-width: 0;
        padding: .85rem 1rem;
        border: 1px solid var(--bs-border-color);
        border-radius: .5rem;
        background: var(--bs-body-bg);
    }

    .auth-meta-label {
        color: var(--bs-secondary-color);
        font-size: .72rem;
        font-weight: 600;
        letter-spacing: .04rem;
        text-transform: uppercase;
    }

    .auth-meta-value {
        min-width: 0;
        overflow: hidden;
        font-weight: 600;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .auth-panel-body {
        display: grid;
        gap: 1.25rem;
        padding: 1.5rem;
    }

    .auth-feedback {
        display: grid;
        gap: .75rem;
    }

    .auth-feedback :global(.alert) {
        margin: 0;
    }

    .auth-section {
        display: grid;
        gap: 1rem;
    }

    .auth-section + .auth-section {
        padding-top: 1.25rem;
        border-top: 1px solid var(--bs-border-color);
    }

    .auth-section-header {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 1rem;
    }

    .auth-section-header h2 {
        margin: 0 0 .25rem;
        font-size: 1rem;
        font-weight: 600;
        line-height: 1.3;
    }

    .auth-section-header p {
        margin: 0;
        color: var(--bs-secondary-color);
        font-size: .9rem;
        line-height: 1.45;
    }

    .auth-form {
        display: grid;
        gap: .95rem;
    }

    .auth-form-otp {
        grid-template-columns: minmax(0, 1fr) auto;
        align-items: start;
    }

    .auth-form :global(.form-group) {
        margin-bottom: 0;
    }

    .auth-form :global(.form-control) {
        min-height: 2.75rem;
    }

    .auth-submit-button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: .5rem;
        min-width: 10rem;
        min-height: 2.75rem;
    }

    .auth-divider {
        display: flex;
        align-items: center;
        gap: .75rem;
        color: var(--bs-secondary-color);
        font-size: .75rem;
        font-weight: 600;
        letter-spacing: .04rem;
        text-transform: uppercase;
    }

    .auth-divider::before,
    .auth-divider::after {
        content: '';
        flex: 1 1 auto;
        height: 1px;
        background: var(--bs-border-color);
    }

    .auth-option-button {
        display: flex;
        align-items: center;
        gap: .75rem;
        width: 100%;
        min-width: 0;
        padding: .9rem 1rem;
        border: 1px solid var(--bs-border-color);
        border-radius: var(--wg-option-radius);
        background: var(--bs-body-bg);
        color: var(--bs-body-color);
        text-align: left;
        transition:
            background-color .12s ease-out,
            border-color .12s ease-out,
            color .12s ease-out,
            box-shadow .12s ease-out,
            transform .08s ease-out;
    }

    .auth-option-button:hover,
    .auth-option-button:focus-visible {
        background: var(--wg-option-hover-bg);
        color: var(--wg-option-hover-color);
    }

    .auth-option-button:focus-visible {
        box-shadow: var(--wg-option-focus-ring);
        outline: none;
    }

    .auth-option-button:active {
        transform: translateY(1px);
    }

    .auth-option-button-copy {
        flex: 1 1 auto;
        min-width: 0;
    }

    .auth-option-button-title {
        display: block;
        min-width: 0;
        overflow: hidden;
        font-weight: 600;
        line-height: 1.3;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .auth-option-button-description {
        display: block;
        margin-top: .15rem;
        color: var(--bs-secondary-color);
        font-size: .82rem;
        line-height: 1.35;
    }

    .auth-option-button-arrow {
        flex: 0 0 auto;
        color: var(--bs-secondary-color);
        font-size: .8rem;
        opacity: .8;
    }

    .auth-option-button:hover .auth-option-button-arrow,
    .auth-option-button:focus-visible .auth-option-button-arrow {
        color: currentColor;
        opacity: 1;
    }

    .sso-buttons {
        display: grid;
        gap: .75rem;
    }

    .sso-button-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: 0 0 auto;
        width: 2rem;
        height: 2rem;
        border-radius: .375rem;
        background: var(--bs-secondary-bg);
        color: var(--bs-body-color);
    }

    .auth-footer {
        display: flex;
        justify-content: flex-end;
        padding-top: .25rem;
    }

    .auth-cancel-button {
        min-width: 10rem;
    }

    @media (max-width: 576px) {
        .auth-panel-header,
        .auth-panel-body {
            padding: 1.1rem;
        }

        .auth-meta-grid {
            grid-template-columns: 1fr;
        }

        .auth-form-otp {
            grid-template-columns: 1fr;
        }

        .auth-submit-button,
        .auth-cancel-button {
            width: 100%;
        }

        .auth-footer {
            justify-content: stretch;
        }
    }
</style>
