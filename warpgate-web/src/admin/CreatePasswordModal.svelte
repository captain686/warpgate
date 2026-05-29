<script lang="ts">
    import {
        Button,
        Form,
        FormGroup,
        Input,
        Modal,
        ModalBody,
        ModalFooter,
    } from '@sveltestrap/sveltestrap'

    import { ResponseError, stringifyError } from './lib/api'
    import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'

    interface Props {
        isOpen: boolean
        create: (password: string) => Promise<void> | void
        actionLabel?: string
        fieldLabel?: string
    }

    let {
        isOpen = $bindable(true),
        create,
        actionLabel = 'Create',
        fieldLabel = 'Enter a new password',
    }: Props = $props()
    let password = $state('')
    let field: HTMLInputElement|undefined = $state()
    let validated = $state(false)
    let errorText: string | null = $state(null)

    async function _save () {
        if (!password.trim()) {
            return
        }
        errorText = null
        try {
            await create(password)
            isOpen = false
            password = ''
            validated = false
        } catch (err) {
            errorText = err instanceof ResponseError
                ? await stringifyError(err)
                : err instanceof Error
                    ? err.message
                    : 'Failed to save password'
        }
    }

    function _cancel () {
        isOpen = false
        password = ''
        validated = false
        errorText = null
    }
</script>

<Modal toggle={_cancel} isOpen={isOpen} on:open={() => {
    password = ''
    validated = false
    errorText = null
    field?.focus()
}}>
    <Form {validated} on:submit={e => {
        void _save()
        e.preventDefault()
    }}>
        <ModalBody>
            <FormGroup floating label={fieldLabel} spacing="0">
                <Input
                    bind:inner={field}
                    type="password"
                    placeholder="New password"
                    required
                    bind:value={password} />
            </FormGroup>
            {#if errorText}
                <Alert color="danger" class="mt-3 mb-0">{errorText}</Alert>
            {/if}
        </ModalBody>
        <ModalFooter>
            <Button
                type="submit"
                class="modal-button"
                color="primary"
                on:click={() => validated = true}
            >{actionLabel}</Button>

            <Button
                type="button"
                class="modal-button"
                color="danger"
                on:click={_cancel}
            >Cancel</Button>
        </ModalFooter>
    </Form>
</Modal>
