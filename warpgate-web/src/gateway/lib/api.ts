import { DefaultApi, Configuration, ResponseError } from './api-client'

const configuration = new Configuration({
    basePath: '/@warpgate/api',
})

export const api = new DefaultApi(configuration)
export * from './api-client'

export async function stringifyError (err: ResponseError): Promise<string> {
    return `API error: ${await err.response.text()}`
}

type KeyAlgorithm = 'ed25519' | 'rsa_sha512'

interface RawPublicKeyCredential {
    id: string
    label: string
    target_id?: string | null
    date_added?: string | null
    last_used?: string | null
    abbreviated?: string
    openssh_public_key?: string
    issued_by_warpgate?: boolean
    expires_at?: string | null
    max_uses?: number | null
    uses_left?: number | null
    revoked_at?: string | null
}

interface RawOtpCredential {
    id: string
    target_id?: string | null
}

export interface SelfServicePublicKeyCredential {
    id: string
    label: string
    targetId?: string
    dateAdded?: Date
    lastUsed?: Date
    abbreviated: string
    opensshPublicKey: string
    issuedByWarpgate: boolean
    expiresAt?: Date
    maxUses?: number
    usesLeft?: number
    revokedAt?: Date
}

export interface SelfServiceOtpCredential {
    id: string
    targetId?: string
}

export interface SelfServiceCredentialsState {
    publicKeys: SelfServicePublicKeyCredential[]
    otp: SelfServiceOtpCredential[]
    ldapLinked: boolean
}

interface RawCredentialsState {
    public_keys: RawPublicKeyCredential[]
    otp: RawOtpCredential[]
    ldap_linked: boolean
}

export interface IssueMyPublicKeyArgs {
    label: string
    targetId?: string
    validForSeconds?: number
    maxUses?: number
    algorithm: KeyAlgorithm
}

export interface IssueMyPublicKeyResult {
    credential: SelfServicePublicKeyCredential
    privateKeyOpenssh: string
}

function optionalDate (value: string | null | undefined): Date | undefined {
    return value ? new Date(value) : undefined
}

function normalizePublicKeyCredential (credential: RawPublicKeyCredential): SelfServicePublicKeyCredential {
    return {
        id: credential.id,
        label: credential.label,
        targetId: credential.target_id ?? undefined,
        dateAdded: optionalDate(credential.date_added),
        lastUsed: optionalDate(credential.last_used),
        abbreviated: credential.abbreviated ?? '',
        opensshPublicKey: credential.openssh_public_key ?? '',
        issuedByWarpgate: credential.issued_by_warpgate ?? false,
        expiresAt: optionalDate(credential.expires_at),
        maxUses: credential.max_uses ?? undefined,
        usesLeft: credential.uses_left ?? undefined,
        revokedAt: optionalDate(credential.revoked_at),
    }
}

function normalizeOtpCredential (credential: RawOtpCredential): SelfServiceOtpCredential {
    return {
        id: credential.id,
        targetId: credential.target_id ?? undefined,
    }
}

async function rawRequest (path: string, init?: RequestInit): Promise<Response> {
    const response = await fetch(`${configuration.basePath}${path}`, init)
    if (!response.ok) {
        throw new ResponseError(response, 'Response returned an error code')
    }
    return response
}

export async function getMyCredentialsForTargetActions (): Promise<SelfServiceCredentialsState> {
    const response = await rawRequest('/profile/credentials')
    const payload = await response.json() as RawCredentialsState
    return {
        publicKeys: payload.public_keys.map(normalizePublicKeyCredential),
        otp: payload.otp.map(normalizeOtpCredential),
        ldapLinked: payload.ldap_linked,
    }
}

export async function issueMyPublicKeyCredential (args: IssueMyPublicKeyArgs): Promise<IssueMyPublicKeyResult> {
    if (!args.targetId) {
        throw new Error('Target is required')
    }
    const response = await rawRequest('/profile/credentials/public-keys/issue', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            label: args.label,
            target_id: args.targetId,
            valid_for_seconds: args.validForSeconds,
            max_uses: args.maxUses,
            algorithm: args.algorithm,
        }),
    })
    const payload = await response.json() as {
        credential: RawPublicKeyCredential
        private_key_openssh: string
    }
    return {
        credential: normalizePublicKeyCredential(payload.credential),
        privateKeyOpenssh: payload.private_key_openssh,
    }
}

export async function revokeMyPublicKeyCredential (credentialId: string): Promise<void> {
    await rawRequest(`/profile/credentials/public-keys/${credentialId}/revoke`, {
        method: 'POST',
    })
}

export async function createMyOtpCredential (secretKey: number[], targetId: string): Promise<SelfServiceOtpCredential> {
    const response = await rawRequest('/profile/credentials/otp', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            secret_key: secretKey,
            target_id: targetId,
        }),
    })
    return normalizeOtpCredential(await response.json() as RawOtpCredential)
}

export async function deleteMyOtpCredential (credentialId: string): Promise<void> {
    await rawRequest(`/profile/credentials/otp/${credentialId}`, {
        method: 'DELETE',
    })
}
