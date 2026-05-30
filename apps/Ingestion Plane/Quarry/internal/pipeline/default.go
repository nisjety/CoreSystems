package pipeline

func NewDefaultChain() *Chain {
	return NewChain(
		NewFingerprintPipeline(),
		NewMetadataPipeline(),
		NewStatsPipeline(),
		NewStoragePipeline(),
	)
}
