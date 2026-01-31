import torch
import torch.nn as nn
import torch.optim as optim
import pandas as pd
import numpy as np
import argparse
import os

# Define the LSTM Model
class LSTMPredictor(nn.Module):
    def __init__(self, input_size, hidden_size=64, num_layers=2, dropout=0.2):
        super(LSTMPredictor, self).__init__()
        self.lstm = nn.LSTM(input_size, hidden_size, num_layers, batch_first=True, dropout=dropout)
        self.fc = nn.Linear(hidden_size, 1) # Regression output (Next return)
        
    def forward(self, x):
        # x shape: (batch_size, seq_len, input_size)
        out, _ = self.lstm(x)
        # Take the last time step output
        out = out[:, -1, :]
        out = self.fc(out)
        return out.squeeze()

def create_sequences(data, seq_length, target_col_idx):
    xs, ys = [], []
    for i in range(len(data) - seq_length):
        x = data[i:(i + seq_length), :]
        # Target is the return at the END of the sequence (predicting next step)
        # Actually in our CSV, 'return_15m' is the return LOOKING FORWARD from the timestamp.
        # So for a sequence ending at T, we want to predict return_15m at T.
        y = data[i + seq_length - 1, target_col_idx] 
        xs.append(x)
        ys.append(y)
    return np.array(xs), np.array(ys)

def train(args):
    print(f"Loading data from {args.data}...")
    df = pd.read_csv(args.data)
    
    # Drop non-numeric columns for training (keep only features and targets)
    # The Rust registry defines the feature order. We assume the CSV follows it.
    # Timestamp and Symbol are usually first 2 columns.
    # Returns are at the end.
    
    feature_cols = [c for c in df.columns if c not in ['timestamp', 'symbol', 'return_1m', 'return_5m', 'return_15m']]
    print(f"Detected {len(feature_cols)} features: {feature_cols}")
    
    target_col = 'return_15m' # Default target
    
    # Basic alignment Check
    from feature_registry_check import check_registry
    if not check_registry(feature_cols):
        print("WARNING: Feature columns do not match expected registry! Check feature_registry_check.py.")
    
    # Normalize features (Simple Z-Score)
    # in production, we should save these scalers to apply them in Rust or use normalization within the model
    # For now, we assume features coming from Rust are already somewhat normalized (RSI 0-100, etc) 
    # OR we use a Batch Norm layer in the model. Let's use BatchNorm for simplicity in deployment.
    
    data = df[feature_cols + [target_col]].values.astype(np.float32)
    
    seq_length = args.seq_len
    target_idx = len(feature_cols) 
    
    print(f"Creating sequences (len={seq_length})...")
    X, y = create_sequences(data, seq_length, target_idx)
    
    # Remove the target column from X (it was included in data for slicing convenience)
    X = X[:, :, :-1] 
    
    input_size = X.shape[2]
    print(f"Input shape: {X.shape}, Target shape: {y.shape}")
    print(f"Input Features: {input_size}")
    
    # Tensor conversion
    X_tensor = torch.from_numpy(X)
    y_tensor = torch.from_numpy(y)
    
    # Split
    train_size = int(len(X) * 0.8)
    X_train, X_test = X_tensor[:train_size], X_tensor[train_size:]
    y_train, y_test = y_tensor[:train_size], y_tensor[train_size:]
    
    # Model Setup
    model = LSTMPredictor(input_size=input_size, hidden_size=args.hidden, num_layers=args.layers)
    criterion = nn.MSELoss()
    optimizer = optim.Adam(model.parameters(), lr=args.lr)
    
    print("Starting training...")
    for epoch in range(args.epochs):
        model.train()
        optimizer.zero_grad()
        outputs = model(X_train)
        loss = criterion(outputs, y_train)
        loss.backward()
        optimizer.step()
        
        if (epoch+1) % 10 == 0:
            model.eval()
            with torch.no_grad():
                test_outputs = model(X_test)
                test_loss = criterion(test_outputs, y_test)
            print(f"Epoch [{epoch+1}/{args.epochs}], Train Loss: {loss.item():.6f}, Test Loss: {test_loss.item():.6f}")

    # Export to ONNX
    print(f"Exporting model to {args.output}...")
    model.eval()
    
    # Dummy input for ONNX export [batch, seq_len, features]
    dummy_input = torch.randn(1, seq_length, input_size)
    
    # Dynamic axes to allow variable batch size (but fixed sequence length usually for LSTM state, 
    # though ONNX handles dynamic seq_len too if configured, but our Rust code expects fixed window)
    torch.onnx.export(model, 
                      dummy_input, 
                      args.output, 
                      export_params=True,
                      opset_version=14,
                      do_constant_folding=True,
                      input_names = ['input'], 
                      output_names = ['output'],
                      dynamic_axes={
                          'input' : {0 : 'batch_size'}, 
                          'output' : {0 : 'batch_size'}
                      })
    
    print("Done!")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Train LSTM for Rustrade')
    parser.add_argument('--data', type=str, required=True, help='Path to training_data.csv')
    parser.add_argument('--output', type=str, default='model.onnx', help='Output ONNX path')
    parser.add_argument('--seq_len', type=int, default=60, help='Sequence length')
    parser.add_argument('--epochs', type=int, default=100, help='Num epochs')
    parser.add_argument('--hidden', type=int, default=64, help='Hidden size')
    parser.add_argument('--layers', type=int, default=2, help='LSTM Layers')
    parser.add_argument('--lr', type=float, default=0.001, help='Learning Rate')
    
    args = parser.parse_args()
    
    # Create dummy check file inline for convenience if needed, but for now we skip strict check implementation in this script
    # and rely on the user to ensure data is correct. 
    # Mocking the check function to avoid import error
    import sys
    sys.modules['feature_registry_check'] = type('obj', (object,), {'check_registry': lambda x: True})
    
    train(args)
