export interface IDeploymentData {
    message: string;
    resources: Array<{
        resource: string;
        version_id: string | void;
        locked: boolean | void;
        env: string;
    }>;
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
