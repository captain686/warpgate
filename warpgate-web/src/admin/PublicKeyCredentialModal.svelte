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

    import { ResponseError, stringifyError, type ExistingPublicKeyCredential } from './lib/api'
    import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'

    interface Props {
        isOpen: boolean
        instance?: ExistingPublicKeyCredential
        save: (label: string, opensshPublicKey: string) => Promise<void> | void
    }

    let {
        isOpen = $bindable(true),
        instance,
        save,
    }: Props = $props()

    let field: HTMLInputElement|undefined = $state()
    let label: string = $state('')
    let opensshPublicKey: string = $state('')
    let validated = $state(false)
    let errorText: string | null = $state(null)

    const PK_REGEX = /^ssh-([\w-]+) [A-Za-z0-9+/=]+( (?<comment>[^ ]+))?$/

    function resetFields () {
        label = instance?.label ?? ''
        opensshPublicKey = instance?.opensshPublicKey ?? ''
        validated = false
        errorText = null
    }

    async function _save () {
        if (!opensshPublicKey || !label) {
            return
        }
        if (opensshPublicKey.includes(' ')) {
            const parts = opensshPublicKey.split(' ').filter(x => x)
            opensshPublicKey = `${parts[0]} ${parts[1]}`
        }
        errorText = null
        try {
            await save(label, opensshPublicKey)
            isOpen = false
        } catch (err) {
            errorText = err instanceof ResponseError
                ? await stringifyError(err)
                : err instanceof Error
                    ? err.message
                    : 'Failed to save SSH key'
        }
    }

    function _cancel () {
        isOpen = false
        errorText = null
    }

    $effect(() => field?.addEventListener('paste', e => {
        const clipboardData = e.clipboardData
        if (clipboardData) {
            const newValue = clipboardData.getData('text')
            onPublicKeyPaste(newValue)
        }
    }))

    function onPublicKeyPaste (newValue: string) {
        const match = PK_REGEX.exec(newValue)
        if (!label && match) {
            label = match.groups?.comment || ''
        }
    }
</script>

<Modal toggle={_cancel} isOpen={isOpen} on:open={() => {
    resetFields()
    field?.focus()
}}>
    <Form {validated} on:submit={e => {
        void _save()
        e.preventDefault()
    }}>
        <ModalBody>
            <FormGroup floating label="Label">
                <Input
                    bind:inner={field}
                    type="text"
                    required
                    bind:value={label} />
            </FormGroup>
            <FormGroup floating label="Public key in OpenSSH format" spacing="0">
                <Input
                    style="font-family: monospace; height: 15rem"
                    bind:inner={field}
                    type="textarea"
                    required
                    placeholder="ssh-XXX YYYYYY"
                    bind:value={opensshPublicKey} />
            </FormGroup>
            {#if errorText}
                <Alert color="danger" class="mt-3 mb-0">{errorText}</Alert>
            {/if}
        </ModalBody>
        <ModalFooter>
            <Button
                type="submit"
                color="primary"
                class="modal-button"
                on:click={() => validated = true}
            >Save</Button>

            <Button
                class="modal-button"
                color="danger"
                on:click={_cancel}
            >Cancel</Button>
        </ModalFooter>
    </Form>
</Modal>
