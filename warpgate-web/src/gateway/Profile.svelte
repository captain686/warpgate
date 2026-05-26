<script lang="ts">
import { getContext } from 'svelte'
import { serverInfo } from 'gateway/lib/store'
import NavListItem from 'common/NavListItem.svelte'

    const getRoutePrefix = getContext<() => string>('warpgate.gatewayRoutePrefix') ?? (() => '')
    const prefixedRoute = (path: string) => `${getRoutePrefix()}${path}`
</script>

<div class="page-summary-bar">
    <h1>{$serverInfo!.username}</h1>
</div>

<NavListItem
    title="API tokens"
    description="Manage your API tokens"
    href={prefixedRoute('/profile/api-tokens')}
/>

{#if $serverInfo}
    {#if $serverInfo.ownCredentialManagementAllowed}
        <NavListItem
            title="Credentials"
            description="Manage your passwords and keys"
            href={prefixedRoute('/profile/credentials')}
        />
    {/if}
{/if}

{#if $serverInfo?.ticketSelfServiceEnabled}
    <NavListItem
        title="Ticket requests"
        description="Request and manage self-service access tickets"
        href={prefixedRoute('/ticket-requests')}
    />
{/if}
