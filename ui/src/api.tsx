export interface IDeploymentData {
    message: string;
    deployments: { resource: string; version_id: string; env: string }[];
}

export function deploy(data: IDeploymentData): Promise<void> {
    const req = new Request("/api/deploy", {
        method: "POST",
        mode: "same-origin",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify(data)
    });
    return fetch(req).then(resp => {
        if (!resp.ok) {
            throw new Error("Request failed");
        }
    });
}
