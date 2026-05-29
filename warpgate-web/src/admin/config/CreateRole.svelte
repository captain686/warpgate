<script lang="ts">
import { api } from 'admin/lib/api'
import AsyncButton from 'common/AsyncButton.svelte'
import { replace } from 'svelte-spa-router'
import { Form, FormGroup } from '@sveltestrap/sveltestrap'
import { stringifyError } from 'common/errors'
import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'
import { adminPermissions } from '../lib/store'

let error: string|null = $state(null)
let name = $state('')

function canCreateRole(): boolean {
    return $adminPermissions.accessRolesCreate
}

async function create () {
    if (!canCreateRole()) {
        return
    }

    try {
        const role = await api.createRole({
            roleDataRequest: {
                name,
                isDefault: false,
            },
        })
        replace(`/config/access-roles/${role.id}`)
    } catch (err) {
        error = await stringifyError(err)
    }
}

</script>

<div class="container-max-md">
    {#if error}
        <Alert color="danger">{error}</Alert>
    {/if}

    {#if canCreateRole()}
        <div class="page-summary-bar">
            <h1>add a role</h1>
        </div>

        <div class="narrow-page">
            <Form>
                <FormGroup floating label="Name">
                    <!-- svelte-ignore a11y_autofocus -->
                    <input class="form-control" bind:value={name} required autofocus />
                </FormGroup>

                <AsyncButton
                    color="primary"
                    click={create}
                >Create role</AsyncButton>
            </Form>
        </div>
    {:else}
        <Alert color="warning">
            You have no permission to create access roles.
        </Alert>
    {/if}
</div>
