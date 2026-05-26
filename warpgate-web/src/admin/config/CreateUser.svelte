<script lang="ts">
import { adminPermissions } from '../lib/store'
import AsyncButton from 'common/AsyncButton.svelte'
import CopyButton from 'common/CopyButton.svelte'
import { replace } from 'svelte-spa-router'
import { Button, Form, FormGroup } from '@sveltestrap/sveltestrap'
import { stringifyError } from 'common/errors'
import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'

let error: string|null = $state(null)
let username = $state('')
let password = $state('')
let createdUserId: string|undefined = $state()
let createdUsername = $state('')
let generatedPassword: string|undefined = $state()

async function create () {
    try {
        error = null
        generatedPassword = undefined
        createdUserId = undefined

        const trimmedUsername = username.trim()
        if (!trimmedUsername) {
            error = 'Username is required'
            return
        }

        const response = await fetch('/@warpgate/admin/api/users', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                username: trimmedUsername,
                password: password.trim() ? password : undefined,
            }),
        })

        if (!response.ok) {
            if (response.status === 409) {
                error = `User "${trimmedUsername}" already exists`
                return
            }
            throw new Error(await response.text())
        }

        const payload = await response.json() as {
            user: { id: string }
            generated_password?: string | null
        }

        if (payload.generated_password) {
            createdUserId = payload.user.id
            createdUsername = trimmedUsername
            generatedPassword = payload.generated_password
            username = ''
            password = ''
            return
        }

        replace(`/config/users/${payload.user.id}`)
    } catch (err) {
        error = await stringifyError(err)
    }
}

</script>

<div class="container-max-md">
    {#if error}
    <Alert color="danger">{error}</Alert>
    {/if}

    {#if generatedPassword}
    <Alert color="success" fade={false}>
        <div class="generated-password-alert">
            <div class="me-auto">
                <strong class="d-block">User {createdUsername} created</strong>
                <span class="d-block mb-2">A random password was generated.</span>
                <code class="generated-password">{generatedPassword}</code>
            </div>
            <div class="d-flex gap-2">
                <CopyButton text={generatedPassword} label="Copy" color="success" outline />
                {#if createdUserId}
                <Button
                    color="primary"
                    on:click={() => replace(`/config/users/${createdUserId}`)}
                >
                    Open user
                </Button>
                {/if}
            </div>
        </div>
    </Alert>
    {/if}

    <div class="page-summary-bar">
        <h1>add a user</h1>
        {#if !$adminPermissions.usersCreate}
            <Alert color="warning">You do not have permission to create users.</Alert>
        {/if}
    </div>
    <div class="narrow-page">
        <Form>
            <FormGroup floating label="Username">
                <input class="form-control" required bind:value={username} />
            </FormGroup>
            <FormGroup floating label="Password (optional)">
                <input class="form-control" type="password" bind:value={password} />
            </FormGroup>

            <AsyncButton
            color="primary"
                click={create}
                disabled={!$adminPermissions.usersCreate}
            >Create user</AsyncButton>
        </Form>
    </div>
</div>

<style lang="scss">
    .generated-password-alert {
        display: flex;
        gap: 1rem;
        align-items: center;
    }

    .generated-password {
        display: inline-block;
        max-width: 100%;
        overflow-wrap: anywhere;
        white-space: normal;
    }

    @media (max-width: 767px) {
        .generated-password-alert {
            align-items: stretch;
            flex-direction: column;
        }
    }
</style>
