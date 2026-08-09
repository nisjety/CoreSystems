package grpc

import (
	"context"
	"fmt"
	"time"

	dockerv1 "github.com/coresystem/integration-gateway/api/proto/docker/v1"
	"github.com/coresystem/integration-gateway/internal/docker"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// DockerServiceServer implements the gRPC DockerService
type DockerServiceServer struct {
	dockerv1.UnimplementedDockerServiceServer
	manager *docker.Manager
}

// NewDockerServiceServer creates a new gRPC Docker service server
func NewDockerServiceServer(manager *docker.Manager) *DockerServiceServer {
	return &DockerServiceServer{
		manager: manager,
	}
}

// ListContainers returns all containers
func (s *DockerServiceServer) ListContainers(ctx context.Context, req *dockerv1.ListContainersRequest) (*dockerv1.ListContainersResponse, error) {
	containers, err := s.manager.ListContainers(ctx)
	if err != nil {
		return nil, err
	}

	protoContainers := make([]*dockerv1.ContainerInfo, len(containers))
	for i, c := range containers {
		protoContainers[i] = convertToProtoContainerInfo(c)
	}

	return &dockerv1.ListContainersResponse{
		Containers: protoContainers,
	}, nil
}

// GetContainer returns details for a specific container
func (s *DockerServiceServer) GetContainer(ctx context.Context, req *dockerv1.GetContainerRequest) (*dockerv1.GetContainerResponse, error) {
	container, err := s.manager.GetContainer(ctx, req.ContainerId)
	if err != nil {
		return nil, err
	}

	return &dockerv1.GetContainerResponse{
		Container: convertToProtoContainerInfo(*container),
	}, nil
}

// StartContainer starts a stopped container
func (s *DockerServiceServer) StartContainer(ctx context.Context, req *dockerv1.StartContainerRequest) (*dockerv1.StartContainerResponse, error) {
	err := s.manager.StartContainer(ctx, req.ContainerId)
	if err != nil {
		return &dockerv1.StartContainerResponse{
			Success: false,
			Message: err.Error(),
		}, nil
	}

	return &dockerv1.StartContainerResponse{
		Success: true,
		Message: fmt.Sprintf("Container %s started successfully", req.ContainerId),
	}, nil
}

// StopContainer stops a running container
func (s *DockerServiceServer) StopContainer(ctx context.Context, req *dockerv1.StopContainerRequest) (*dockerv1.StopContainerResponse, error) {
	timeout := int(req.TimeoutSeconds)
	if timeout == 0 {
		timeout = 10 // Default timeout
	}

	err := s.manager.StopContainer(ctx, req.ContainerId, timeout)
	if err != nil {
		return &dockerv1.StopContainerResponse{
			Success: false,
			Message: err.Error(),
		}, nil
	}

	return &dockerv1.StopContainerResponse{
		Success: true,
		Message: fmt.Sprintf("Container %s stopped successfully", req.ContainerId),
	}, nil
}

// RestartContainer restarts a container
func (s *DockerServiceServer) RestartContainer(ctx context.Context, req *dockerv1.RestartContainerRequest) (*dockerv1.RestartContainerResponse, error) {
	timeout := int(req.TimeoutSeconds)
	if timeout == 0 {
		timeout = 10
	}

	err := s.manager.RestartContainer(ctx, req.ContainerId, timeout)
	if err != nil {
		return &dockerv1.RestartContainerResponse{
			Success: false,
			Message: err.Error(),
		}, nil
	}

	return &dockerv1.RestartContainerResponse{
		Success: true,
		Message: fmt.Sprintf("Container %s restarted successfully", req.ContainerId),
	}, nil
}

// GetContainerLogs retrieves logs from a container
func (s *DockerServiceServer) GetContainerLogs(ctx context.Context, req *dockerv1.GetContainerLogsRequest) (*dockerv1.GetContainerLogsResponse, error) {
	tail := fmt.Sprintf("%d", req.Tail)
	since := fmt.Sprintf("%d", req.SinceUnix)

	logs, err := s.manager.GetContainerLogs(ctx, req.ContainerId, tail, since)
	if err != nil {
		return nil, err
	}

	return &dockerv1.GetContainerLogsResponse{
		Logs: logs,
	}, nil
}

// GetContainerStats returns real-time statistics for a container
func (s *DockerServiceServer) GetContainerStats(ctx context.Context, req *dockerv1.GetContainerStatsRequest) (*dockerv1.GetContainerStatsResponse, error) {
	stats, err := s.manager.GetContainerStats(ctx, req.ContainerId)
	if err != nil {
		return nil, err
	}

	return &dockerv1.GetContainerStatsResponse{
		Stats: convertToProtoContainerStats(*stats),
	}, nil
}

// ListImages returns all Docker images
func (s *DockerServiceServer) ListImages(ctx context.Context, req *dockerv1.ListImagesRequest) (*dockerv1.ListImagesResponse, error) {
	images, err := s.manager.ListImages(ctx)
	if err != nil {
		return nil, err
	}

	protoImages := make([]*dockerv1.ImageInfo, len(images))
	for i, img := range images {
		created := time.Unix(img.Created, 0)
		protoImages[i] = &dockerv1.ImageInfo{
			Id:          img.ID,
			RepoTags:    img.RepoTags,
			RepoDigests: img.RepoDigests,
			Created:     timestamppb.New(created),
			Size:        img.Size,
			VirtualSize: img.VirtualSize,
			Labels:      img.Labels,
		}
	}

	return &dockerv1.ListImagesResponse{
		Images: protoImages,
	}, nil
}

// ListNetworks returns all Docker networks
func (s *DockerServiceServer) ListNetworks(ctx context.Context, req *dockerv1.ListNetworksRequest) (*dockerv1.ListNetworksResponse, error) {
	networks, err := s.manager.ListNetworks(ctx)
	if err != nil {
		return nil, err
	}

	protoNetworks := make([]*dockerv1.NetworkInfo, len(networks))
	for i, net := range networks {
		protoNetworks[i] = &dockerv1.NetworkInfo{
			Id:         net.ID,
			Name:       net.Name,
			Driver:     net.Driver,
			Scope:      net.Scope,
			Internal:   net.Internal,
			Attachable: net.Attachable,
			Labels:     net.Labels,
		}
	}

	return &dockerv1.ListNetworksResponse{
		Networks: protoNetworks,
	}, nil
}

// ListVolumes returns all Docker volumes
func (s *DockerServiceServer) ListVolumes(ctx context.Context, req *dockerv1.ListVolumesRequest) (*dockerv1.ListVolumesResponse, error) {
	volumes, err := s.manager.ListVolumes(ctx)
	if err != nil {
		return nil, err
	}

	protoVolumes := make([]*dockerv1.VolumeInfo, len(volumes))
	for i, vol := range volumes {
		// Parse CreatedAt time
		created, _ := time.Parse(time.RFC3339, vol.CreatedAt)

		protoVolumes[i] = &dockerv1.VolumeInfo{
			Name:       vol.Name,
			Driver:     vol.Driver,
			Mountpoint: vol.Mountpoint,
			Created:    timestamppb.New(created),
			Scope:      vol.Scope,
			Labels:     vol.Labels,
		}
	}

	return &dockerv1.ListVolumesResponse{
		Volumes: protoVolumes,
	}, nil
}

// GetSystemInfo returns Docker system information
func (s *DockerServiceServer) GetSystemInfo(ctx context.Context, req *dockerv1.GetSystemInfoRequest) (*dockerv1.GetSystemInfoResponse, error) {
	info, err := s.manager.GetSystemInfo(ctx)
	if err != nil {
		return nil, err
	}

	return &dockerv1.GetSystemInfoResponse{
		Version:           info.ServerVersion,
		ApiVersion:        "", // API version not exposed in system.Info
		Os:                info.OperatingSystem,
		Architecture:      info.Architecture,
		Containers:        int32(info.Containers),
		ContainersRunning: int32(info.ContainersRunning),
		ContainersPaused:  int32(info.ContainersPaused),
		ContainersStopped: int32(info.ContainersStopped),
		Images:            int32(info.Images),
		Driver:            info.Driver,
		MemoryTotal:       info.MemTotal,
	}, nil
}

// GetAquatiqServices returns all Aquatiq-prefixed services
func (s *DockerServiceServer) GetAquatiqServices(ctx context.Context, req *dockerv1.GetAquatiqServicesRequest) (*dockerv1.GetAquatiqServicesResponse, error) {
	services, err := s.manager.GetAquatiqServices(ctx)
	if err != nil {
		return nil, err
	}

	protoServices := make([]*dockerv1.ContainerInfo, len(services))
	for i, svc := range services {
		protoServices[i] = convertToProtoContainerInfo(svc)
	}

	return &dockerv1.GetAquatiqServicesResponse{
		Services: protoServices,
	}, nil
}

// Helper functions

func convertToProtoContainerInfo(c docker.ContainerInfo) *dockerv1.ContainerInfo {
	ports := make([]*dockerv1.PortBinding, len(c.Ports))
	for i, p := range c.Ports {
		ports[i] = &dockerv1.PortBinding{
			PrivatePort: int32(p.PrivatePort),
			PublicPort:  int32(p.PublicPort),
			Type:        p.Type,
			Ip:          "", // Docker API doesn't always provide IP in list
		}
	}

	created := time.Unix(c.Created, 0)

	return &dockerv1.ContainerInfo{
		Id:      c.ID,
		Names:   []string{c.Name},
		Image:   c.Image,
		ImageId: "", // Short info doesn't include imageID
		Command: "",
		Created: timestamppb.New(created),
		State:   c.State,
		Status:  c.Status,
		Ports:   ports,
		Labels:  c.Labels,
	}
}

func convertToProtoContainerStats(s docker.ContainerStats) *dockerv1.ContainerStats {
	return &dockerv1.ContainerStats{
		ContainerId:      s.ID,
		Name:             s.Name,
		CpuPercentage:    s.CPUPercent,
		MemoryUsage:      int64(s.MemoryUsage),
		MemoryLimit:      int64(s.MemoryLimit),
		MemoryPercentage: s.MemoryPercent,
		NetworkRx:        int64(s.NetworkRx),
		NetworkTx:        int64(s.NetworkTx),
		BlockRead:        int64(s.BlockRead),
		BlockWrite:       int64(s.BlockWrite),
		Pids:             0, // Not provided in current implementation
	}
}
