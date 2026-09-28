package server

import mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"

type AcquireLeaseRequest = mpv1.AcquireLeaseRequest
type AcquireLeaseResponse = mpv1.AcquireLeaseResponse
type ReleaseLeaseRequest = mpv1.ReleaseLeaseRequest
type ReleaseLeaseResponse = mpv1.ReleaseLeaseResponse
type SnapshotRequest = mpv1.SnapshotRequest
type SnapshotResponse = mpv1.SnapshotResponse
type ActivateLeaseRequest = mpv1.ActivateLeaseRequest
type ActivateLeaseResponse = mpv1.ActivateLeaseResponse
type GetWorkspaceManifestRequest = mpv1.GetWorkspaceManifestRequest
type GetWorkspaceManifestResponse = mpv1.GetWorkspaceManifestResponse
type WorkspaceManifestEntry = mpv1.WorkspaceManifestEntry
type WorkspaceChangedFile = mpv1.WorkspaceChangedFile
type PromoteWorkspaceRequest = mpv1.PromoteWorkspaceRequest
type PromoteWorkspaceResponse = mpv1.PromoteWorkspaceResponse
type SandboxHealthRequest = mpv1.SandboxHealthRequest
type SandboxHealthResponse = mpv1.SandboxHealthResponse

// S4.2 process registry.
type RegisterProcessRequest = mpv1.RegisterProcessRequest
type RegisterProcessResponse = mpv1.RegisterProcessResponse
type UpdateProcessStateRequest = mpv1.UpdateProcessStateRequest
type UpdateProcessStateResponse = mpv1.UpdateProcessStateResponse
type AppendProcessOutputRequest = mpv1.AppendProcessOutputRequest
type AppendProcessOutputResponse = mpv1.AppendProcessOutputResponse
type ReconcileProcessesRequest = mpv1.ReconcileProcessesRequest
type ReconcileProcessesResponse = mpv1.ReconcileProcessesResponse
type GetProcessRequest = mpv1.GetProcessRequest
type GetProcessResponse = mpv1.GetProcessResponse
type ListProcessesRequest = mpv1.ListProcessesRequest
type ListProcessesResponse = mpv1.ListProcessesResponse
type ReadProcessOutputRequest = mpv1.ReadProcessOutputRequest
type ReadProcessOutputResponse = mpv1.ReadProcessOutputResponse
type Process = mpv1.Process
type ProcessOutputChunk = mpv1.ProcessOutputChunk
type RedactedCommand = mpv1.RedactedCommand
